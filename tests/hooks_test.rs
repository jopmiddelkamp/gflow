mod common;

use std::path::{Path, PathBuf};

use common::{tmp_dir, MockGit, MockHookShell, MockPrompter, TempDir};
use gflow::hooks::{self, decide, rerun_line, HookEnv, HookEvent, Hooks, Step, TagEvent};
use gflow::version::SemVer;

const WEB: &str = "deploy-web staging";
const APP: &str = "build-app";

struct World {
    git: MockGit,
    home: TempDir,
    shell: MockHookShell,
}

impl World {
    fn new() -> Self {
        Self { git: MockGit::with_tmp_git_dir("gflow-hooks-git"), home: tmp_dir("gflow-hooks-home"), shell: MockHookShell::new() }
    }

    fn approvals(&self) -> PathBuf {
        self.home.join(".gflow").join(hooks::APPROVALS_FILE)
    }

    fn env(&self, commands: &[(HookEvent, String)], interactive: bool) -> HookEnv<'_> {
        HookEnv { commands: commands.to_vec(), shell: &self.shell, interactive, approvals_file: Some(self.approvals()) }
    }

    fn approve(&self, commands: &[(HookEvent, String)]) {
        for (_, command) in commands {
            hooks::approve(&self.approvals(), &self.git.repo_root, command).unwrap();
        }
    }

    fn ledger_dir(&self) -> PathBuf {
        self.git.git_dir.join(hooks::LEDGER_DIR)
    }
}

fn cmds(entries: &[(HookEvent, &str)]) -> Vec<(HookEvent, String)> {
    entries.iter().map(|(e, c)| (*e, c.to_string())).collect()
}

fn bump_event() -> TagEvent {
    TagEvent::new(HookEvent::ReleaseBump, &SemVer::parse("v1.2.0-rc.2").unwrap(), "release/1.2.0")
}

fn env_of(run: &common::HookRun) -> Vec<(&str, &str)> {
    run.env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect()
}

#[test]
fn each_event_has_one_key_named_after_it() {
    let keys: Vec<String> = HookEvent::ALL.iter().map(|e| e.key()).collect();
    assert_eq!(keys, ["hook-release-start", "hook-release-bump", "hook-release-finish", "hook-hotfix-finish"]);
    for event in HookEvent::ALL {
        assert_eq!(hooks::parse_entry(&event.key(), " run it ").unwrap(), (event, "run it".to_string()));
    }
}

#[test]
fn an_unknown_hook_key_is_refused_with_the_valid_keys() {
    let err = hooks::parse_entry("hook-relase-bump", "deploy").unwrap_err();
    assert_eq!(
        err,
        "Unknown key 'hook-relase-bump' in .gflow/config. Valid hook keys: hook-release-start, hook-release-bump, hook-release-finish, hook-hotfix-finish."
    );
}

#[test]
fn an_empty_hook_command_is_refused() {
    let err = hooks::parse_entry("hook-release-bump", "  ").unwrap_err();
    assert_eq!(err, "Empty command for 'hook-release-bump' in .gflow/config.");
}

#[test]
fn the_variables_name_the_event_tag_kind_branch_and_remote() {
    assert_eq!(bump_event().env(), vec![
        ("GFLOW_EVENT", "release-bump".to_string()),
        ("GFLOW_TAG", "v1.2.0-rc.2".to_string()),
        ("GFLOW_TAG_KIND", "rc".to_string()),
        ("GFLOW_BRANCH", "release/1.2.0".to_string()),
        ("GFLOW_REMOTE", "origin".to_string()),
    ]);
    let finish = TagEvent::new(HookEvent::HotfixFinish, &SemVer::new(1, 2, 1), "hotfix/1.2.1");
    assert_eq!(finish.env()[0].1, "hotfix-finish");
    assert_eq!(finish.env()[1].1, "v1.2.1");
    assert_eq!(finish.env()[2].1, "final");
}

#[test]
fn the_rerun_line_fills_in_every_variable_and_quotes_for_the_shell() {
    let env = bump_event().env();
    assert_eq!(
        rerun_line("echo 'hi' $GFLOW_TAG", Path::new("/repos/my app"), &env, false),
        "cd '/repos/my app' && GFLOW_EVENT='release-bump' GFLOW_TAG='v1.2.0-rc.2' GFLOW_TAG_KIND='rc' \
         GFLOW_BRANCH='release/1.2.0' GFLOW_REMOTE='origin' sh -c 'echo '\\''hi'\\'' $GFLOW_TAG'"
    );
    assert_eq!(
        rerun_line("deploy.cmd", Path::new("C:\\repos\\app"), &env, true),
        "cd /d \"C:\\repos\\app\" && set \"GFLOW_EVENT=release-bump\" && set \"GFLOW_TAG=v1.2.0-rc.2\" && \
         set \"GFLOW_TAG_KIND=rc\" && set \"GFLOW_BRANCH=release/1.2.0\" && set \"GFLOW_REMOTE=origin\" && cmd /C \"deploy.cmd\""
    );
}

#[test]
fn the_firing_decision_covers_every_combination() {
    for approved in [false, true] {
        for interactive in [false, true] {
            assert_eq!(decide(true, approved, interactive), Step::AlreadyRan);
        }
    }
    assert_eq!(decide(false, true, false), Step::Run);
    assert_eq!(decide(false, true, true), Step::Run);
    assert_eq!(decide(false, false, true), Step::Ask);
    assert_eq!(decide(false, false, false), Step::SkipUnapproved);
}

#[test]
fn no_hook_keys_means_no_hooks_and_no_git_calls() {
    let world = World::new();
    let env = world.env(&[], true);
    assert!(Hooks::prepare(&world.git, &env).unwrap().is_none());
    assert!(world.git.calls().is_empty());
}

#[test]
fn an_event_runs_its_own_commands_in_file_order_with_its_variables_and_nothing_else() {
    let world = World::new();
    let commands = cmds(&[
        (HookEvent::ReleaseStart, "start-only"),
        (HookEvent::ReleaseBump, WEB),
        (HookEvent::ReleaseFinish, "finish-only"),
        (HookEvent::ReleaseBump, APP),
    ]);
    world.approve(&commands);
    let env = world.env(&commands, false);
    let hooks = Hooks::prepare(&world.git, &env).unwrap().unwrap();
    assert_eq!(world.git.calls(), vec!["repo_root", "git_common_dir"]);

    hooks.record(&bump_event(), false).unwrap();
    hooks.fire_pending(&MockPrompter::new()).unwrap();

    assert_eq!(world.shell.commands(), vec![WEB, APP]);
    for run in world.shell.runs.borrow().iter() {
        assert_eq!(run.cwd, world.git.repo_root);
        assert_eq!(env_of(run), vec![
            ("GFLOW_EVENT", "release-bump"),
            ("GFLOW_TAG", "v1.2.0-rc.2"),
            ("GFLOW_TAG_KIND", "rc"),
            ("GFLOW_BRANCH", "release/1.2.0"),
            ("GFLOW_REMOTE", "origin"),
        ]);
    }
}

#[test]
fn an_event_without_a_key_runs_nothing() {
    let world = World::new();
    let commands = cmds(&[(HookEvent::ReleaseStart, "start-only")]);
    world.approve(&commands);
    let env = world.env(&commands, false);
    let hooks = Hooks::prepare(&world.git, &env).unwrap().unwrap();

    hooks.record(&bump_event(), false).unwrap();
    hooks.fire_pending(&MockPrompter::new()).unwrap();

    assert!(world.shell.commands().is_empty());
}

#[test]
fn a_failing_command_does_not_stop_the_next_and_the_run_fails_at_the_end() {
    let mut world = World::new();
    world.shell.outcomes.insert(WEB.to_string(), Ok(Some(3)));
    world.shell.outcomes.insert("missing-tool".to_string(), Ok(Some(127)));
    world.shell.outcomes.insert("killed".to_string(), Ok(None));
    world.shell.outcomes.insert("no-shell".to_string(), Err("failed to start sh: not found".to_string()));
    let commands = cmds(&[
        (HookEvent::ReleaseBump, WEB),
        (HookEvent::ReleaseBump, "missing-tool"),
        (HookEvent::ReleaseBump, "killed"),
        (HookEvent::ReleaseBump, "no-shell"),
        (HookEvent::ReleaseBump, APP),
    ]);
    world.approve(&commands);
    let env = world.env(&commands, false);
    let hooks = Hooks::prepare(&world.git, &env).unwrap().unwrap();
    hooks.record(&bump_event(), false).unwrap();

    let err = hooks.fire_pending(&MockPrompter::new()).unwrap_err();

    assert_eq!(world.shell.commands(), vec![WEB, "missing-tool", "killed", "no-shell", APP]);
    assert_eq!(err, "4 hook commands failed. The release-bump of v1.2.0-rc.2 itself is complete; rerun each failed command shown above.");
    // At most once: a failed command is not retried by a later run.
    hooks.fire_pending(&MockPrompter::new()).unwrap();
    assert_eq!(world.shell.runs.borrow().len(), 5);
}

#[test]
fn the_failure_line_names_the_exit_code_and_the_rerun_command() {
    let env = bump_event().env();
    let root = Path::new("/repos/app");
    assert_eq!(
        hooks::failure_line(&Ok(Some(3)), WEB, root, &env, false),
        format!("✖ Hook failed (exit 3). Rerun: {}", rerun_line(WEB, root, &env, false))
    );
    assert_eq!(
        hooks::failure_line(&Ok(None), WEB, root, &env, false),
        format!("✖ Hook failed (killed by a signal). Rerun: {}", rerun_line(WEB, root, &env, false))
    );
    assert_eq!(
        hooks::failure_line(&Err("no sh".to_string()), WEB, root, &env, false),
        format!("✖ Hook failed (no sh). Rerun: {}", rerun_line(WEB, root, &env, false))
    );
    assert_eq!(hooks::failure_line(&Ok(Some(0)), WEB, root, &env, false), "");
}

#[test]
fn one_failure_is_reported_in_the_singular() {
    let mut world = World::new();
    world.shell.outcomes.insert(WEB.to_string(), Ok(Some(1)));
    let commands = cmds(&[(HookEvent::ReleaseBump, WEB)]);
    world.approve(&commands);
    let env = world.env(&commands, false);
    let hooks = Hooks::prepare(&world.git, &env).unwrap().unwrap();
    hooks.record(&bump_event(), false).unwrap();

    let err = hooks.fire_pending(&MockPrompter::new()).unwrap_err();

    assert!(err.starts_with("1 hook command failed."), "got: {err}");
}

#[test]
fn a_recorded_event_fires_once_even_when_recorded_again() {
    let world = World::new();
    let commands = cmds(&[(HookEvent::ReleaseBump, WEB)]);
    world.approve(&commands);
    let env = world.env(&commands, false);
    let hooks = Hooks::prepare(&world.git, &env).unwrap().unwrap();

    hooks.record(&bump_event(), false).unwrap();
    hooks.fire_pending(&MockPrompter::new()).unwrap();
    hooks.record(&bump_event(), false).unwrap();
    hooks.fire_pending(&MockPrompter::new()).unwrap();

    assert_eq!(world.shell.commands(), vec![WEB]);
}

#[test]
fn a_crash_after_the_flow_but_before_the_hooks_fires_every_command_once_on_the_next_run() {
    let world = World::new();
    let commands = cmds(&[(HookEvent::ReleaseBump, WEB), (HookEvent::ReleaseBump, APP)]);
    world.approve(&commands);
    let env = world.env(&commands, false);
    Hooks::prepare(&world.git, &env).unwrap().unwrap().record(&bump_event(), false).unwrap();
    // The process died here: a fresh run only finds the ledger.

    let next_run = Hooks::prepare(&world.git, &env).unwrap().unwrap();
    next_run.fire_pending(&MockPrompter::new()).unwrap();
    next_run.fire_pending(&MockPrompter::new()).unwrap();

    assert_eq!(world.shell.commands(), vec![WEB, APP]);
}

#[test]
fn a_crash_between_two_commands_runs_each_command_exactly_once_overall() {
    let mut world = World::new();
    world.shell.crash_after = Some(1);
    let commands = cmds(&[(HookEvent::ReleaseBump, WEB), (HookEvent::ReleaseBump, APP)]);
    world.approve(&commands);
    {
        let env = world.env(&commands, false);
        let hooks = Hooks::prepare(&world.git, &env).unwrap().unwrap();
        hooks.record(&bump_event(), false).unwrap();
        let crashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| hooks.fire_pending(&MockPrompter::new())));
        assert!(crashed.is_err());
    }
    world.shell.crash_after = None;

    let env = world.env(&commands, false);
    let resumed = Hooks::prepare(&world.git, &env).unwrap().unwrap();
    resumed.fire_pending(&MockPrompter::new()).unwrap();
    resumed.fire_pending(&MockPrompter::new()).unwrap();

    assert_eq!(world.shell.commands(), vec![WEB, APP]);
}

#[test]
fn the_ledger_marks_a_command_before_it_runs() {
    let mut world = World::new();
    world.shell.crash_after = Some(1);
    let commands = cmds(&[(HookEvent::ReleaseBump, WEB)]);
    world.approve(&commands);
    {
        let env = world.env(&commands, false);
        let hooks = Hooks::prepare(&world.git, &env).unwrap().unwrap();
        hooks.record(&bump_event(), false).unwrap();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| hooks.fire_pending(&MockPrompter::new())));
    }
    let ledger = std::fs::read_to_string(world.ledger_dir().join("release-bump-v1.2.0-rc.2.state")).unwrap();
    assert!(ledger.contains(&format!("ran={}", gflow::sha256::hex_digest(WEB.as_bytes()))), "got: {ledger}");
    assert!(ledger.contains("status=pending"), "got: {ledger}");
}

#[test]
fn pending_events_fire_oldest_first() {
    let world = World::new();
    let commands = cmds(&[(HookEvent::ReleaseStart, "start"), (HookEvent::ReleaseBump, "bump")]);
    world.approve(&commands);
    let env = world.env(&commands, false);
    let hooks = Hooks::prepare(&world.git, &env).unwrap().unwrap();
    hooks.record(&TagEvent::new(HookEvent::ReleaseStart, &SemVer::parse("v1.2.0-rc.1").unwrap(), "release/1.2.0"), false).unwrap();
    hooks.record(&bump_event(), false).unwrap();

    hooks.fire_pending(&MockPrompter::new()).unwrap();

    assert_eq!(world.shell.commands(), vec!["start", "bump"]);
}

#[test]
fn no_hooks_records_the_event_as_handled_and_runs_nothing() {
    let world = World::new();
    let commands = cmds(&[(HookEvent::ReleaseBump, WEB)]);
    world.approve(&commands);
    let env = world.env(&commands, false);
    let hooks = Hooks::prepare(&world.git, &env).unwrap().unwrap();

    hooks.record(&bump_event(), true).unwrap();
    hooks.fire_pending(&MockPrompter::new()).unwrap();

    assert!(world.shell.commands().is_empty());
}

#[test]
fn an_unapproved_command_asks_and_yes_runs_it_and_remembers_the_answer() {
    let world = World::new();
    let commands = cmds(&[(HookEvent::ReleaseBump, WEB)]);
    let env = world.env(&commands, true);
    let hooks = Hooks::prepare(&world.git, &env).unwrap().unwrap();
    hooks.record(&bump_event(), false).unwrap();
    let prompter = MockPrompter::new().with_lines(&["Y"]);

    hooks.fire_pending(&prompter).unwrap();

    assert_eq!(prompter.calls(), vec![format!("prompt_line:Hook for release-bump: {WEB}\nRun this hook? [y/N]")]);
    assert_eq!(world.shell.commands(), vec![WEB]);
    assert!(hooks::is_approved(&world.approvals(), &world.git.repo_root, WEB).unwrap());
}

#[test]
fn answering_no_skips_the_command_and_asks_again_next_time() {
    let world = World::new();
    let commands = cmds(&[(HookEvent::ReleaseBump, WEB)]);
    let env = world.env(&commands, true);
    let hooks = Hooks::prepare(&world.git, &env).unwrap().unwrap();
    hooks.record(&bump_event(), false).unwrap();

    hooks.fire_pending(&MockPrompter::new().with_lines(&[""])).unwrap();

    assert!(world.shell.commands().is_empty());
    assert!(!hooks::is_approved(&world.approvals(), &world.git.repo_root, WEB).unwrap());
}

#[test]
fn an_aborted_prompt_stops_the_run() {
    let world = World::new();
    let commands = cmds(&[(HookEvent::ReleaseBump, WEB)]);
    let env = world.env(&commands, true);
    let hooks = Hooks::prepare(&world.git, &env).unwrap().unwrap();
    hooks.record(&bump_event(), false).unwrap();

    assert_eq!(hooks.fire_pending(&MockPrompter::aborting()), Err("Aborted".to_string()));
    assert!(world.shell.commands().is_empty());
}

#[test]
fn without_a_terminal_an_unapproved_command_is_skipped() {
    let world = World::new();
    let commands = cmds(&[(HookEvent::ReleaseBump, WEB)]);
    let env = world.env(&commands, false);
    let hooks = Hooks::prepare(&world.git, &env).unwrap().unwrap();
    hooks.record(&bump_event(), false).unwrap();
    let prompter = MockPrompter::new();

    hooks.fire_pending(&prompter).unwrap();

    assert!(world.shell.commands().is_empty());
    assert!(prompter.calls().is_empty());
}

#[test]
fn a_changed_command_needs_a_new_approval() {
    let world = World::new();
    world.approve(&cmds(&[(HookEvent::ReleaseBump, WEB)]));
    let changed = format!("{WEB} --force");

    assert!(hooks::is_approved(&world.approvals(), &world.git.repo_root, WEB).unwrap());
    assert!(!hooks::is_approved(&world.approvals(), &world.git.repo_root, &changed).unwrap());
}

#[test]
fn an_approval_is_scoped_to_its_repository() {
    let world = World::new();
    world.approve(&cmds(&[(HookEvent::ReleaseBump, WEB)]));

    assert!(!hooks::is_approved(&world.approvals(), Path::new("/repos/other"), WEB).unwrap());
}

#[test]
fn without_a_home_directory_nothing_is_approved_or_stored() {
    let world = World::new();
    let commands = cmds(&[(HookEvent::ReleaseBump, WEB)]);
    let env = HookEnv { commands: commands.clone(), shell: &world.shell, interactive: true, approvals_file: None };
    let hooks = Hooks::prepare(&world.git, &env).unwrap().unwrap();
    hooks.record(&bump_event(), false).unwrap();

    hooks.fire_pending(&MockPrompter::new().with_lines(&["y"])).unwrap();

    assert_eq!(world.shell.commands(), vec![WEB]);
    assert!(!world.approvals().exists());
}

#[test]
fn a_corrupt_ledger_is_an_error_that_names_the_file() {
    let world = World::new();
    let commands = cmds(&[(HookEvent::ReleaseBump, WEB)]);
    std::fs::create_dir_all(world.ledger_dir()).unwrap();
    let file = world.ledger_dir().join("release-bump-v1.2.0-rc.2.state");
    std::fs::write(&file, "garbage\n").unwrap();
    let env = world.env(&commands, false);
    let hooks = Hooks::prepare(&world.git, &env).unwrap().unwrap();

    let err = hooks.fire_pending(&MockPrompter::new()).unwrap_err();

    assert!(err.contains(&file.display().to_string()), "got: {err}");
}

#[test]
fn preparing_fails_when_git_cannot_answer() {
    let commands = cmds(&[(HookEvent::ReleaseBump, WEB)]);
    for call in ["repo_root", "git_common_dir"] {
        let mut world = World::new();
        world.git.fail_call = Some((call.to_string(), 1));
        let env = world.env(&commands, false);
        assert!(Hooks::prepare(&world.git, &env).is_err(), "{call}");
    }
}

#[cfg(unix)]
#[test]
fn the_shell_adapter_runs_in_the_given_folder_with_the_variables_and_reports_the_outcome() {
    use gflow::hooks::{HookShell, ShellHook};
    let dir = tmp_dir("gflow-hooks-shell");
    let env = bump_event().env();

    let exited = ShellHook.run("printf '%s %s' \"$GFLOW_EVENT\" \"$GFLOW_TAG\" > out.txt; exit 3", &dir, &env);
    let killed = ShellHook.run("kill -9 $$", &dir, &env);
    let unstartable = ShellHook.run("true", &dir.join("missing"), &env).unwrap_err();

    assert_eq!(exited, Ok(Some(3)));
    assert_eq!(std::fs::read_to_string(dir.join("out.txt")).unwrap(), "release-bump v1.2.0-rc.2");
    assert_eq!(killed, Ok(None));
    assert!(unstartable.starts_with("could not start the shell: "), "got: {unstartable}");
}

#[test]
fn an_unreadable_approvals_file_stops_before_any_command_runs() {
    let world = World::new();
    let commands = cmds(&[(HookEvent::ReleaseBump, WEB)]);
    std::fs::create_dir_all(world.approvals()).unwrap();
    let env = world.env(&commands, true);
    let hooks = Hooks::prepare(&world.git, &env).unwrap().unwrap();
    hooks.record(&bump_event(), false).unwrap();

    let err = hooks.fire_pending(&MockPrompter::new()).unwrap_err();

    assert!(err.starts_with(&format!("Failed to read {}", world.approvals().display())), "got: {err}");
    assert!(world.shell.commands().is_empty());
}

#[test]
fn an_approval_that_cannot_be_saved_stops_before_the_command_runs() {
    let world = World::new();
    let commands = cmds(&[(HookEvent::ReleaseBump, WEB)]);
    std::fs::write(world.home.join(".gflow"), "a file where the folder belongs").unwrap();
    let env = world.env(&commands, true);
    let hooks = Hooks::prepare(&world.git, &env).unwrap().unwrap();
    hooks.record(&bump_event(), false).unwrap();

    let err = hooks.fire_pending(&MockPrompter::new().with_lines(&["y"])).unwrap_err();

    assert!(err.starts_with(&format!("Failed to write {}", world.approvals().display())), "got: {err}");
    assert!(world.shell.commands().is_empty());
}

#[test]
fn a_blocked_ledger_folder_fails_recording_and_firing() {
    let world = World::new();
    let commands = cmds(&[(HookEvent::ReleaseBump, WEB)]);
    world.approve(&commands);
    std::fs::write(world.ledger_dir(), "a file where the folder belongs").unwrap();
    let env = world.env(&commands, false);
    let hooks = Hooks::prepare(&world.git, &env).unwrap().unwrap();

    let recorded = hooks.record(&bump_event(), false).unwrap_err();
    let fired = hooks.fire_pending(&MockPrompter::new()).unwrap_err();

    assert!(recorded.starts_with("Failed to write "), "got: {recorded}");
    assert!(fired.starts_with(&format!("Failed to read {}", world.ledger_dir().display())), "got: {fired}");
    assert!(world.shell.commands().is_empty());
}

#[test]
fn an_unreadable_ledger_entry_is_an_error_that_names_it() {
    let world = World::new();
    let commands = cmds(&[(HookEvent::ReleaseBump, WEB)]);
    let entry = world.ledger_dir().join("release-bump-v1.2.0-rc.2.state");
    std::fs::create_dir_all(&entry).unwrap();
    let env = world.env(&commands, false);
    let hooks = Hooks::prepare(&world.git, &env).unwrap().unwrap();

    let err = hooks.fire_pending(&MockPrompter::new()).unwrap_err();

    assert!(err.starts_with(&format!("Failed to read {}", entry.display())), "got: {err}");
}

#[cfg(unix)]
#[test]
fn a_ledger_that_cannot_be_updated_stops_before_its_command_runs() {
    use std::os::unix::fs::PermissionsExt;
    let world = World::new();
    let commands = cmds(&[(HookEvent::ReleaseBump, WEB)]);
    world.approve(&commands);
    let env = world.env(&commands, false);
    let hooks = Hooks::prepare(&world.git, &env).unwrap().unwrap();
    hooks.record(&bump_event(), false).unwrap();
    let ledger = world.ledger_dir().join("release-bump-v1.2.0-rc.2.state");
    std::fs::set_permissions(&ledger, std::fs::Permissions::from_mode(0o444)).unwrap();

    let err = hooks.fire_pending(&MockPrompter::new()).unwrap_err();

    assert!(err.starts_with(&format!("Failed to write {}", ledger.display())), "got: {err}");
    assert!(world.shell.commands().is_empty(), "at most once: no run without its mark");
}

#[cfg(unix)]
#[test]
fn a_ledger_that_cannot_be_closed_is_an_error() {
    use std::os::unix::fs::PermissionsExt;
    let world = World::new();
    let commands = cmds(&[(HookEvent::ReleaseStart, "start-only")]);
    let env = world.env(&commands, false);
    let hooks = Hooks::prepare(&world.git, &env).unwrap().unwrap();
    hooks.record(&bump_event(), false).unwrap();
    let ledger = world.ledger_dir().join("release-bump-v1.2.0-rc.2.state");
    std::fs::set_permissions(&ledger, std::fs::Permissions::from_mode(0o444)).unwrap();

    let err = hooks.fire_pending(&MockPrompter::new()).unwrap_err();

    assert!(err.starts_with(&format!("Failed to write {}", ledger.display())), "got: {err}");
}

mod common;

use std::path::{Path, PathBuf};

use common::MockCommandRunner;
use gflow::git::{BranchDivergence, CliOutput, CommandRunner, Git, GitCli, RunningCommand};

// `GitCli` is the git adapter. Two things in it are worth pinning:
//
//   * the decisions it derives from git's output — exit-code semantics ("false"
//     is never conflated with "failed", per decisions.md Error Model) and the
//     output parsers that the flow mocks have always faked, including
//     `find_stash_by_message`, which is what makes "never a blind pop" real
//   * the flags each primitive passes (`merge --no-ff`, `stash push -u`,
//     `branch -D`), pinned as one table at the bottom of this file
//
// Process fixtures cover `SystemRunner` separately without touching real git.

fn git(runner: &MockCommandRunner) -> GitCli<'_> {
    GitCli::new(runner)
}

// --- Background fetch ---

/// Runs like the mock but cannot start a process in the background.
struct SpawnFails(MockCommandRunner);

impl CommandRunner for SpawnFails {
    fn run(&self, program: &str, args: &[&str]) -> Result<CliOutput, String> {
        self.0.run(program, args)
    }
    fn spawn(&self, _: &str, _: &[&str], _: &[(&str, &str)]) -> Result<Box<dyn RunningCommand + '_>, String> {
        Err("no processes left".into())
    }
}

#[test]
fn a_started_fetch_is_awaited_instead_of_fetching_twice() {
    let runner = MockCommandRunner::scripted(&[(1, "", ""), (0, "", "")]);
    let git = git(&runner);

    git.start_fetch();
    git.fetch().unwrap();

    // BatchMode: ssh fails instead of asking for a passphrase over the menu.
    assert_eq!(runner.calls(), vec![
        "git config --get core.sshCommand",
        "spawn GIT_TERMINAL_PROMPT=0 GIT_SSH_COMMAND=ssh -o BatchMode=yes: git fetch --all --prune --quiet",
        "wait: git fetch --all --prune --quiet",
    ]);
}

#[test]
fn an_unclaimed_background_fetch_finishes_on_its_own() {
    // An aborted menu exits at once instead of waiting seconds for the fetch;
    // the fetch is never killed, so it cannot leave .lock files behind.
    let runner = MockCommandRunner::scripted(&[(1, "", ""), (0, "", "")]);

    git(&runner).start_fetch();

    assert!(runner.calls().last().unwrap().starts_with("spawn "), "calls: {:?}", runner.calls());
}

#[test]
fn a_failed_background_fetch_is_retried_where_prompts_work() {
    let runner = MockCommandRunner::scripted(&[(1, "", ""), (128, "", "Permission denied (publickey)"), (0, "", "")]);
    let git = git(&runner);

    git.start_fetch();
    git.fetch().unwrap();

    assert_eq!(runner.calls().last().unwrap(), "git fetch --all --prune --quiet");
}

#[test]
fn a_background_fetch_keeps_the_users_own_ssh_command() {
    let runner = MockCommandRunner::scripted(&[(0, "ssh -i ~/.ssh/work\n", ""), (0, "", "")]);
    let git = git(&runner);

    git.start_fetch();

    assert_eq!(runner.calls()[1],
        "spawn GIT_TERMINAL_PROMPT=0 GIT_SSH_COMMAND=ssh -i ~/.ssh/work -o BatchMode=yes: git fetch --all --prune --quiet");
}

#[test]
fn without_a_background_fetch_the_fetch_runs_in_the_foreground() {
    // An unreadable ssh setting or a failed spawn: nothing is started, and the
    // fetch behaves as if it never was.
    let unreadable = MockCommandRunner::scripted(&[(2, "", "bad config"), (0, "", "")]);
    let git = git(&unreadable);
    git.start_fetch();
    git.fetch().unwrap();
    assert_eq!(unreadable.calls(), vec!["git config --get core.sshCommand", "git fetch --all --prune --quiet"]);

    let spawn_fails = SpawnFails(MockCommandRunner::scripted(&[(1, "", ""), (0, "", "")]));
    let git = GitCli::new(&spawn_fails);
    git.start_fetch();
    git.fetch().unwrap();
    assert_eq!(spawn_fails.0.calls(), vec!["git config --get core.sshCommand", "git fetch --all --prune --quiet"]);
}

// --- Exit-code semantics ---

#[test]
fn a_failed_command_reports_the_command_and_gits_own_stderr() {
    let runner = MockCommandRunner::scripted(&[(1, "", "fatal: not a valid ref\n")]);

    let err = git(&runner).checkout("nope").unwrap_err();

    assert_eq!(err, "git checkout nope failed: fatal: not a valid ref");
}

#[test]
fn a_check_command_maps_exit_zero_and_one_to_true_and_false() {
    // `merge-base --is-ancestor` answers a question with its exit code. Exit 1 is
    // "no", not a failure — every idempotent finish step depends on this.
    let yes = MockCommandRunner::scripted(&[(0, "", "")]);
    assert!(git(&yes).is_ancestor("release/2.5.0", "main").unwrap());

    let no = MockCommandRunner::scripted(&[(1, "", "")]);
    assert!(!git(&no).is_ancestor("release/2.5.0", "main").unwrap());
}

#[test]
fn a_check_command_still_treats_other_exit_codes_as_failures() {
    // Exit 128 (bad ref, not a repo) must not silently read as "false" — that
    // would make a finish skip a merge that never happened.
    let runner = MockCommandRunner::scripted(&[(128, "", "fatal: bad revision\n")]);

    let err = git(&runner).is_ancestor("nope", "main").unwrap_err();

    assert!(err.contains("exit 128"), "got: {err}");
    assert!(err.contains("fatal: bad revision"), "got: {err}");
}

#[test]
fn a_command_killed_by_a_signal_is_a_failure_not_a_false() {
    let runner = MockCommandRunner::terminated_by_signal();

    let err = git(&runner).is_ancestor("a", "b").unwrap_err();

    assert!(err.contains("terminated by signal"), "got: {err}");
}

#[test]
fn an_unset_config_key_reads_as_none_rather_than_an_error() {
    // `git config --get` exits 1 when the key is absent. Every gflow.* default
    // depends on that being "unset", not "git broke".
    let runner = MockCommandRunner::scripted(&[(1, "", "")]);

    assert_eq!(git(&runner).get_config("gflow.worktree.enabled").unwrap(), None);
}

#[test]
fn a_scoped_section_read_returns_every_key_of_that_scope_in_one_call() {
    // `get_config` returns the *effective* value, which cannot tell a local
    // override from a global default. The config migration has to know which
    // file a value belongs in, so it reads each scope explicitly.
    let runner = MockCommandRunner::scripted(&[(0, "gflow.worktree.editor code --wait\ngflow.worktree.enabled\n", "")]);

    let values = git(&runner).config_section_at("gflow.worktree", true).unwrap();

    assert_eq!(values, vec![
        ("gflow.worktree.editor".to_string(), "code --wait".to_string()),
        ("gflow.worktree.enabled".to_string(), String::new()),
    ]);
    assert_eq!(runner.calls(), vec![r"git config --global --get-regexp ^gflow\.worktree\."]);
}

#[test]
fn a_scoped_section_read_of_the_local_scope_never_falls_back_to_global() {
    let runner = MockCommandRunner::scripted(&[(1, "", "")]);

    let values = git(&runner).config_section_at("gflow.worktree", false).unwrap();

    assert_eq!(values, Vec::new(), "unset in this scope means unset, not inherited");
    assert_eq!(runner.calls(), vec![r"git config --local --get-regexp ^gflow\.worktree\."]);
}

#[test]
fn an_unreadable_config_section_is_an_error_not_an_empty_section() {
    let runner = MockCommandRunner::scripted(&[(2, "", "bad config line 3")]);

    let err = git(&runner).config_section_at("gflow.worktree", true).unwrap_err();

    assert!(err.contains("bad config line 3"), "got: {err}");
}

#[test]
fn a_set_config_key_reads_back_trimmed() {
    let runner = MockCommandRunner::scripted(&[(0, "  cursor \n", "")]);

    assert_eq!(git(&runner).get_config("gflow.worktree.editor").unwrap(), Some("cursor".to_string()));
}

#[test]
fn a_broken_config_read_is_still_an_error() {
    let runner = MockCommandRunner::scripted(&[(128, "", "fatal: not in a git directory\n")]);

    assert!(git(&runner).get_config("gflow.worktree.editor").is_err());
}

#[test]
fn unsetting_an_already_unset_key_succeeds() {
    // `git config --unset` exits 5 when the key was not set. "Use the default"
    // must be idempotent — running it twice is not an error.
    let runner = MockCommandRunner::scripted(&[(5, "", "")]);

    git(&runner).unset_config("gflow.worktree.path", true).unwrap();

    assert_eq!(runner.calls(), vec!["git config --global --unset gflow.worktree.path"]);
}

#[test]
fn unset_config_still_fails_on_a_real_error() {
    let runner = MockCommandRunner::scripted(&[(4, "", "error: cannot lock config file\n")]);

    let err = git(&runner).unset_config("gflow.worktree.path", false).unwrap_err();

    assert!(err.contains("exit 4"), "got: {err}");
}

// --- Output parsers ---

#[test]
fn tag_lists_drop_blank_lines() {
    let runner = MockCommandRunner::ok("v1.0.0\nv1.1.0\n\n");

    assert_eq!(git(&runner).list_tags().unwrap(), vec!["v1.0.0", "v1.1.0"]);
}

#[test]
fn branch_lists_strip_the_origin_prefix_and_are_sorted_and_deduped() {
    // A branch that exists both locally and on origin appears twice in
    // `for-each-ref` output; the trait contract promises one sorted entry.
    let runner = MockCommandRunner::ok("origin/release/2.5.0\nrelease/2.5.0\norigin/release/2.4.0\n");

    let branches = git(&runner).list_branches_matching("release/*").unwrap();

    assert_eq!(branches, vec!["release/2.4.0", "release/2.5.0"]);
}

#[test]
fn remote_branch_lists_drop_the_origin_head_pointer() {
    // `origin/HEAD` is a symbolic ref, not a branch — offering it as a PR target
    // would be nonsense.
    let runner = MockCommandRunner::ok("origin/HEAD\norigin/develop\norigin/feature/x\n");

    assert_eq!(git(&runner).list_remote_branches().unwrap(), vec!["develop", "feature/x"]);
}

#[test]
fn remote_branch_divergence_reads_both_counts_for_every_branch_in_one_call() {
    let runner = MockCommandRunner::ok("HEAD 0 0\ndevelop 9 5\nfeature/x 0 2\n");

    let divergence = git(&runner).remote_branch_divergence("feature/child").unwrap();

    assert_eq!(divergence, vec![
        BranchDivergence { branch: "develop".into(), ahead: 9, behind: 5 },
        BranchDivergence { branch: "feature/x".into(), ahead: 0, behind: 2 },
    ], "origin/HEAD is a pointer, not a branch");
    assert_eq!(runner.calls(), vec![
        "git for-each-ref --format=%(refname:lstrip=3) %(ahead-behind:feature/child) refs/remotes/origin/",
    ]);
}

#[test]
fn remote_branch_divergence_rejects_output_it_cannot_read() {
    for line in ["develop 9", "develop nine 5", "develop 9 five"] {
        let runner = MockCommandRunner::ok(line);

        let err = git(&runner).remote_branch_divergence("feature/child").unwrap_err();

        assert_eq!(err, format!("Unexpected ahead-behind data from git: '{line}'"));
    }
}

#[test]
fn tags_on_branch_drops_blank_lines() {
    let runner = MockCommandRunner::ok("v2.5.0-rc.1\nv2.5.0-rc.2\n");

    assert_eq!(git(&runner).tags_on_branch("release/2.5.0").unwrap(), vec!["v2.5.0-rc.1", "v2.5.0-rc.2"]);
}

#[test]
fn a_commit_count_is_parsed_as_a_number() {
    let runner = MockCommandRunner::ok("3\n");

    assert_eq!(git(&runner).rev_list_count("v2.5.0-rc.1", "release/2.5.0").unwrap(), 3);
    assert_eq!(runner.calls(), vec!["git rev-list --count v2.5.0-rc.1..release/2.5.0"],
        "the two refs become one range argument");
}

#[test]
fn parent_count_of_a_merge_commit_is_two() {
    // `rev-list --parents -n 1` prints the commit followed by its parents.
    let runner = MockCommandRunner::ok("deadbeef aaa111 bbb222\n");

    assert_eq!(git(&runner).commit_parent_count("deadbeef").unwrap(), 2);
    assert_eq!(runner.calls(), vec!["git rev-list --parents -n 1 deadbeef"]);
}

#[test]
fn parent_count_of_a_squash_commit_is_one() {
    let runner = MockCommandRunner::ok("deadbeef aaa111\n");

    assert_eq!(git(&runner).commit_parent_count("deadbeef").unwrap(), 1);
}

#[test]
fn parent_count_empty_output_is_an_error_not_a_zero() {
    // Zero parents is a root commit; empty output means the commit is unknown —
    // guessing either way would let a completion-type check pass vacuously.
    let runner = MockCommandRunner::ok("");

    let err = git(&runner).commit_parent_count("deadbeef").unwrap_err();

    assert!(err.contains("deadbeef"), "must name the commit; got: {err}");
}

#[test]
fn an_unparseable_commit_count_is_an_error_not_a_zero() {
    // Zero would mean "nothing past the RC" and would wave the release gate through.
    let runner = MockCommandRunner::ok("not-a-number");

    let err = git(&runner).rev_list_count("a", "b").unwrap_err();

    assert!(err.contains("Failed to parse rev-list count"), "got: {err}");
}

#[test]
fn commit_messages_are_split_on_nul_so_bodies_survive() {
    // Multi-line commit bodies are why the separator is NUL and not a newline —
    // breaking-change footers live on their own lines inside one message.
    let runner = MockCommandRunner::ok("feat!: drop v1\n\nBREAKING CHANGE: gone\n\0fix: typo\n\0");

    let messages = git(&runner).commit_messages("v1.0.0", "develop").unwrap();

    assert_eq!(messages, vec!["feat!: drop v1\n\nBREAKING CHANGE: gone", "fix: typo"]);
}

#[test]
fn a_clean_working_tree_is_empty_porcelain_output() {
    let clean = MockCommandRunner::ok("");
    assert!(git(&clean).is_working_tree_clean().unwrap());

    let dirty = MockCommandRunner::ok(" M src/main.rs\n");
    assert!(!git(&dirty).is_working_tree_clean().unwrap());
}

#[test]
fn unmerged_paths_are_detected_from_porcelain_conflict_markers() {
    for marker in ["UU", "AA", "DD", "AU", "UA", "DU", "UD"] {
        let runner = MockCommandRunner::ok(&format!("{marker} src/main.rs\n"));
        assert!(git(&runner).has_unmerged_paths().unwrap(), "marker {marker} must count as a conflict");
    }
}

#[test]
fn ordinary_modifications_are_not_unmerged_paths() {
    // A dirty tree is not a conflicted tree — conflating them would send the user
    // to "resolve conflicts, run git commit" for uncommitted work.
    let runner = MockCommandRunner::ok(" M src/main.rs\n?? notes.txt\nA  new.rs\n");

    assert!(!git(&runner).has_unmerged_paths().unwrap());
}

#[test]
fn a_tag_push_reports_whether_origin_received_the_tag() {
    // --porcelain flags are not translated: `=` is "already up to date" in
    // every locale, which is what lets the push double as its own guard.
    let new = MockCommandRunner::ok("To origin\n*\trefs/tags/v2.5.0:refs/tags/v2.5.0\t[new tag]\nDone\n");
    assert!(git(&new).push_tag("v2.5.0").unwrap());

    let present = MockCommandRunner::ok("To origin\n=\trefs/tags/v2.5.0:refs/tags/v2.5.0\t[up to date]\nDone\n");
    assert!(!git(&present).push_tag("v2.5.0").unwrap());
}

#[test]
fn a_branch_counts_as_pushed_when_both_shas_match() {
    let runner = MockCommandRunner::scripted(&[(0, "abc123\n", ""), (0, "abc123\n", "")]);

    assert!(git(&runner).is_pushed("main").unwrap());
    assert_eq!(runner.calls(), vec![
        "git rev-parse main",
        "git rev-parse refs/remotes/origin/main",
    ]);
}

#[test]
fn a_branch_never_pushed_is_not_pushed_rather_than_an_error() {
    // The remote ref does not resolve at all — normal for a brand-new branch.
    let runner = MockCommandRunner::scripted(&[(0, "abc123\n", ""), (128, "", "fatal: bad revision\n")]);

    assert!(!git(&runner).is_pushed("feature/new").unwrap());
}

#[test]
fn a_branch_behind_origin_is_not_pushed() {
    let runner = MockCommandRunner::scripted(&[(0, "local-sha\n", ""), (0, "remote-sha\n", "")]);

    assert!(!git(&runner).is_pushed("main").unwrap());
}

// --- Worktree resolution ---

#[test]
fn the_repo_root_is_the_first_worktree_git_lists() {
    // `git worktree list` always lists the main working tree first, so this
    // resolves the same root from inside any linked worktree.
    let runner = MockCommandRunner::ok(
        "worktree /repos/app\nHEAD abc\nbranch refs/heads/develop\n\n\
         worktree /repos/app-feature-x\nHEAD def\nbranch refs/heads/feature/x\n");

    assert_eq!(git(&runner).repo_root().unwrap(), PathBuf::from("/repos/app"));
}

#[test]
fn unreadable_worktree_output_is_an_error_not_a_guess() {
    let runner = MockCommandRunner::ok("something unexpected\n");

    let err = git(&runner).repo_root().unwrap_err();

    assert!(err.contains("Could not determine the main working tree"), "got: {err}");
}

#[test]
fn a_linked_worktree_is_one_whose_git_dir_differs_from_the_common_dir() {
    let linked = MockCommandRunner::ok("/repos/app/.git/worktrees/x\n/repos/app/.git\n");
    assert!(git(&linked).is_linked_worktree().unwrap());

    let main = MockCommandRunner::ok("/repos/app/.git\n/repos/app/.git\n");
    assert!(!git(&main).is_linked_worktree().unwrap());
}

#[test]
fn a_single_line_from_rev_parse_is_an_error() {
    // Guessing here would risk running `worktree remove` against the main checkout.
    let runner = MockCommandRunner::ok("/repos/app/.git\n");

    let err = git(&runner).is_linked_worktree().unwrap_err();

    assert!(err.contains("Unexpected"), "got: {err}");
}

#[test]
fn removing_the_current_worktree_runs_from_the_main_working_tree() {
    // git refuses to remove the worktree it is running in, so the removal is
    // issued with -C <main root> and returns the path that was removed.
    let runner = MockCommandRunner::scripted(&[
        (0, "/repos/app-feature-x\n", ""),                 // rev-parse --show-toplevel
        (0, "worktree /repos/app\nHEAD abc\n", ""),        // worktree list (repo_root)
        (0, "", ""),                                        // the removal itself
    ]);

    let removed = git(&runner).remove_current_worktree().unwrap();

    assert_eq!(removed, PathBuf::from("/repos/app-feature-x"));
    assert_eq!(runner.calls()[2],
        "git -C /repos/app worktree remove --force /repos/app-feature-x");
}

// --- Stash lookup: the mechanism behind "never a blind pop" ---

#[test]
fn a_stash_is_found_by_its_message_and_returns_its_ref() {
    let runner = MockCommandRunner::ok(
        "malformed\n\
         stash@{0} On develop: someone else's work\n\
         stash@{1} On develop: gflow-finish:release/2.5.0:1700000000\n");

    let found = git(&runner).find_stash_by_message("gflow-finish:release/2.5.0:1700000000").unwrap();

    assert_eq!(found, Some("stash@{1}".to_string()),
        "the index is looked up, never assumed — stash@{{0}} here belongs to the user");
}

#[test]
fn an_absent_stash_message_yields_none_rather_than_a_guess() {
    let runner = MockCommandRunner::ok("stash@{0} On develop: unrelated work\n");

    assert_eq!(git(&runner).find_stash_by_message("gflow-finish:release/2.5.0:1").unwrap(), None);
}

#[test]
fn the_git_dir_is_read_as_a_path() {
    let runner = MockCommandRunner::ok(".git\n");

    assert_eq!(git(&runner).git_dir().unwrap(), PathBuf::from(".git"));
}

// --- The exact git invocation each primitive makes ---
//
// These are one-line pass-throughs, but their *flags* are decisions, not
// incidentals: `merge --no-ff` is what keeps release merges visible in history,
// `stash push -u` is what makes untracked files survive a finish, `branch -D`
// force-deletes a branch gflow has already confirmed is merged. A silent flag
// change is a behavior change, and nothing else in the suite would catch it.

#[test]
fn every_primitive_issues_its_documented_git_command() {
    let cases: Vec<(&str, Box<dyn Fn(&GitCli<'_>)>)> = vec![
        ("git rev-parse --abbrev-ref HEAD", Box::new(|g| { g.current_branch().ok(); })),
        // --prune so deleted remote branches stop showing up as PR targets.
        ("git fetch --all --prune --quiet", Box::new(|g| { g.fetch().ok(); })),
        ("git checkout develop", Box::new(|g| { g.checkout("develop").ok(); })),
        ("git checkout -b feature/x develop", Box::new(|g| { g.create_branch("feature/x", "develop").ok(); })),
        ("git branch feature/x develop", Box::new(|g| { g.create_branch_no_checkout("feature/x", "develop").ok(); })),
        // -u sets the upstream, so the branch is comparable with origin afterwards.
        ("git push -u origin feature/x", Box::new(|g| { g.push("feature/x").ok(); })),
        ("git push --porcelain origin v2.5.0", Box::new(|g| { g.push_tag("v2.5.0").ok(); })),
        // Annotated (-a) tags carry an author and message; releases are annotated.
        ("git tag -a v2.5.0 -m chore: release 2.5.0", Box::new(|g| { g.create_tag("v2.5.0", "chore: release 2.5.0").ok(); })),
        // --no-ff keeps the merge commit: the gitflow history must show the merge.
        ("git merge release/2.5.0 --no-ff -m chore: merge", Box::new(|g| { g.merge("release/2.5.0", "chore: merge").ok(); })),
        // --ff-only is the opposite intent: sync, never invent a merge commit.
        ("git merge origin/develop --ff-only", Box::new(|g| { g.ff_merge("origin/develop").ok(); })),
        ("git tag --list", Box::new(|g| { g.list_tags().ok(); })),
        ("git tag --merged release/2.5.0", Box::new(|g| { g.tags_on_branch("release/2.5.0").ok(); })),
        // -D force-deletes: gflow only calls this once the branch is verifiably merged.
        ("git branch -D feature/x", Box::new(|g| { g.delete_branch_local("feature/x").ok(); })),
        ("git push origin --delete feature/x feature/y", Box::new(|g| { g.delete_remote_branches(&["feature/x", "feature/y"]).ok(); })),
        ("git status --porcelain", Box::new(|g| { g.is_working_tree_clean().ok(); })),
        ("git for-each-ref --format=%(refname:short) refs/remotes/origin/", Box::new(|g| { g.list_remote_branches().ok(); })),
        // Both patterns: a release branch that exists only on origin must still
        // be discoverable.
        ("git for-each-ref --format=%(refname:short) refs/remotes/origin/release/* refs/heads/release/*",
            Box::new(|g| { g.list_branches_matching("release/*").ok(); })),
        ("git merge-base a b", Box::new(|g| { g.merge_base("a", "b").ok(); })),
        ("git merge-base --is-ancestor a b", Box::new(|g| { g.is_ancestor("a", "b").ok(); })),
        // --verify --quiet is what makes show-ref answer via exit code alone.
        ("git show-ref --verify --quiet refs/tags/v2.5.0", Box::new(|g| { g.tag_exists("v2.5.0").ok(); })),
        ("git show-ref --verify --quiet refs/heads/feature/x", Box::new(|g| { g.local_branch_exists("feature/x").ok(); })),
        ("git show-ref --verify --quiet refs/remotes/origin/feature/x", Box::new(|g| { g.remote_branch_exists("feature/x").ok(); })),
        // %x00 is load-bearing: the parser splits on NUL, the one byte a commit
        // message cannot contain. %B%n would split multi-paragraph messages.
        ("git log v2.5.0..develop --format=%B%x00", Box::new(|g| { g.commit_messages("v2.5.0", "develop").ok(); })),
        ("git rev-parse --git-dir", Box::new(|g| { g.git_dir().ok(); })),
        ("git remote get-url origin", Box::new(|g| { g.remote_url().ok(); })),
        ("git config --get gflow.worktree.editor", Box::new(|g| { g.get_config("gflow.worktree.editor").ok(); })),
        ("git config --global gflow.worktree.editor code", Box::new(|g| { g.set_config("gflow.worktree.editor", "code", true).ok(); })),
        ("git config gflow.worktree.editor code", Box::new(|g| { g.set_config("gflow.worktree.editor", "code", false).ok(); })),
        ("git worktree add /repos/app-feature-x feature/x", Box::new(|g| { g.add_worktree(Path::new("/repos/app-feature-x"), "feature/x").ok(); })),
        // -C runs the same primitives in the tree that already holds the target
        // branch; the flags must stay identical to the current-tree variants.
        ("git -C /repos/app status --porcelain", Box::new(|g| { g.is_working_tree_clean_at(Path::new("/repos/app")).ok(); })),
        ("git -C /repos/app merge origin/develop --ff-only", Box::new(|g| { g.ff_merge_at(Path::new("/repos/app"), "origin/develop").ok(); })),
        ("git -C /repos/app merge hotfix/1.0.1 --no-ff -m chore: merge", Box::new(|g| { g.merge_at(Path::new("/repos/app"), "hotfix/1.0.1", "chore: merge").ok(); })),
        ("git rev-parse --git-dir --git-common-dir", Box::new(|g| { g.is_linked_worktree().ok(); })),
        ("git rev-parse HEAD", Box::new(|g| { g.head_sha().ok(); })),
        ("git checkout --detach", Box::new(|g| { g.detach_head().ok(); })),
        // -u includes untracked files, so a finish never strands new files.
        ("git stash push -u -m gflow-finish:develop:1", Box::new(|g| { g.stash_push_with_message("gflow-finish:develop:1").ok(); })),
        ("git stash list --format=%gd %s", Box::new(|g| { g.find_stash_by_message("x").ok(); })),
        ("git stash pop stash@{1}", Box::new(|g| { g.stash_pop_ref("stash@{1}").ok(); })),
        ("git add -A", Box::new(|g| { g.stage_all().ok(); })),
        ("git commit -m chore: set version 2.5.0", Box::new(|g| { g.commit("chore: set version 2.5.0").ok(); })),
        // A separate method from create_tag: it tags a specific sha (a merge
        // commit), not HEAD.
        ("git tag -a v2.5.0 -m chore: release 2.5.0 abc123",
            Box::new(|g| { g.create_tag_at("v2.5.0", "chore: release 2.5.0", "abc123").ok(); })),
        // ^{commit} dereferences an annotated tag to its commit (trap 10) —
        // plain rev-parse would return the tag object's own SHA.
        ("git rev-parse v2.5.0^{commit}", Box::new(|g| { g.tag_commit_sha("v2.5.0").ok(); })),
        ("git rev-parse refs/heads/release/2.5.0", Box::new(|g| { g.branch_sha("release/2.5.0").ok(); })),
        // Resolves the CURRENT worktree's root (unlike repo_root, which always
        // resolves the main tree) — this is what lets repo-content resolution
        // (config, version script, PR templates) see the branch you're on.
        ("git rev-parse --show-toplevel", Box::new(|g| { g.worktree_root().ok(); })),
    ];

    for (expected, call) in cases {
        let runner = MockCommandRunner::ok("");
        call(&GitCli::new(&runner));
        assert_eq!(runner.calls(), vec![expected]);
    }
}

#[test]
fn tag_commit_sha_returns_the_trimmed_commit_sha() {
    let runner = MockCommandRunner::ok("  abc123def456  \n");

    assert_eq!(git(&runner).tag_commit_sha("v2.5.0").unwrap(), "abc123def456");
}

#[test]
fn a_mid_merge_repo_is_detected_from_the_marker_files_git_leaves() {
    // Unlike the rest, this reads the filesystem under .git rather than asking
    // git — each marker is a different interrupted operation, and all of them
    // must block a finish.
    use common::tmp_dir;

    for marker in ["MERGE_HEAD", "CHERRY_PICK_HEAD", "REVERT_HEAD", "rebase-merge", "rebase-apply"] {
        let dir = tmp_dir("gflow-midmerge");
        let runner = MockCommandRunner::ok(dir.to_str().unwrap());
        std::fs::write(dir.join(marker), b"").unwrap();

        assert!(GitCli::new(&runner).is_mid_merge().unwrap(), "{marker} must block a finish");
    }
}

#[test]
fn a_repo_with_no_interrupted_operation_is_not_mid_merge() {
    use common::tmp_dir;
    let dir = tmp_dir("gflow-midmerge");
    let runner = MockCommandRunner::ok(dir.to_str().unwrap());

    assert!(!GitCli::new(&runner).is_mid_merge().unwrap());
}

#[test]
fn worktree_of_names_the_tree_that_has_the_branch_checked_out() {
    let runner = MockCommandRunner::ok(
        "worktree /repos/app\nHEAD abc\nbranch refs/heads/develop\n\n\
         worktree /repos/app-feature-x\nHEAD def\nbranch refs/heads/feature/x\n");

    assert_eq!(git(&runner).worktree_of("feature/x").unwrap(), Some(PathBuf::from("/repos/app-feature-x")));
}

#[test]
fn worktree_of_is_none_when_no_tree_holds_the_branch() {
    let runner = MockCommandRunner::ok(
        "worktree /repos/app\nHEAD abc\nbranch refs/heads/develop\n\n\
         worktree /repos/app-fix\nHEAD def\ndetached\n");

    assert_eq!(git(&runner).worktree_of("main").unwrap(), None);
}

#[cfg(unix)]
#[test]
fn invalid_worktree_path_encoding_stops_before_running_git() {
    use std::os::unix::ffi::OsStrExt;
    let path = std::path::Path::new(std::ffi::OsStr::from_bytes(b"/worktrees/\xff"));
    let runner = MockCommandRunner::scripted(&[]);
    let git = GitCli::new(&runner);
    for result in [
        git.is_working_tree_clean_at(path).map(|_| ()),
        git.ff_merge_at(path, "develop"),
        git.merge_at(path, "develop", "merge into develop"),
    ] {
        assert!(result.unwrap_err().contains("Path is not valid UTF-8"));
    }
    assert!(runner.calls().is_empty());
}

struct CannotSpawn;

impl CommandRunner for CannotSpawn {
    fn run(&self, _: &str, _: &[&str]) -> Result<CliOutput, String> {
        Err("git executable unavailable".into())
    }
    fn spawn(&self, _: &str, _: &[&str], _: &[(&str, &str)]) -> Result<Box<dyn RunningCommand + '_>, String> {
        Err("git executable unavailable".into())
    }
}

type GitRead = fn(&dyn Git) -> Result<(), String>;

#[test]
fn failed_git_reads_never_become_empty_or_clean_results() {
    let cases: &[(&str, GitRead)] = &[
        ("git tag --list", |g| g.list_tags().map(|_| ())),
        ("git for-each-ref --format=%(refname:short) refs/remotes/origin/release/* refs/heads/release/*", |g| g.list_branches_matching("release/*").map(|_| ())),
        ("git status --porcelain", |g| g.is_working_tree_clean().map(|_| ())),
        ("git for-each-ref --format=%(refname:short) refs/remotes/origin/", |g| g.list_remote_branches().map(|_| ())),
        ("git for-each-ref --format=%(refname:lstrip=3) %(ahead-behind:a) refs/remotes/origin/", |g| g.remote_branch_divergence("a").map(|_| ())),
        ("git rev-list --count a..b", |g| g.rev_list_count("a", "b").map(|_| ())),
        ("git rev-list --parents -n 1 a", |g| g.commit_parent_count("a").map(|_| ())),
        ("git log a..b --format=%B%x00", |g| g.commit_messages("a", "b").map(|_| ())),
        ("git push --porcelain origin v1.0.0", |g| g.push_tag("v1.0.0").map(|_| ())),
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

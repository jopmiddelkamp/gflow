mod common;

use std::path::{Path, PathBuf};

use common::MockCommandRunner;
use gflow::git::{Git, GitCli};

// `GitCli` is the git adapter. Two things in it are worth pinning:
//
//   * the decisions it derives from git's output — exit-code semantics ("false"
//     is never conflated with "failed", per decisions.md Error Model) and the
//     output parsers that the flow mocks have always faked, including
//     `find_stash_by_message`, which is what makes "never a blind pop" real
//   * the flags each primitive passes (`merge --no-ff`, `stash push -u`,
//     `branch -D`), pinned as one table at the bottom of this file
//
// The process spawn itself (`SystemRunner`) stays untested by design: tests
// never touch real git (SKILL.md principle 9).

fn git(runner: &MockCommandRunner) -> GitCli<'_> {
    GitCli::new(runner)
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
fn a_remote_tag_exists_when_ls_remote_prints_anything() {
    let found = MockCommandRunner::ok("abc123\trefs/tags/v2.5.0\n");
    assert!(git(&found).remote_tag_exists("v2.5.0").unwrap());

    let missing = MockCommandRunner::ok("");
    assert!(!git(&missing).remote_tag_exists("v2.5.0").unwrap());
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
        "stash@{0} On develop: someone else's work\n\
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
        ("git fetch --all --prune", Box::new(|g| { g.fetch().ok(); })),
        ("git checkout develop", Box::new(|g| { g.checkout("develop").ok(); })),
        ("git checkout -b feature/x develop", Box::new(|g| { g.create_branch("feature/x", "develop").ok(); })),
        ("git branch feature/x develop", Box::new(|g| { g.create_branch_no_checkout("feature/x", "develop").ok(); })),
        // -u sets the upstream, so the branch is comparable with origin afterwards.
        ("git push -u origin feature/x", Box::new(|g| { g.push("feature/x").ok(); })),
        ("git push origin v2.5.0", Box::new(|g| { g.push_tag("v2.5.0").ok(); })),
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
        ("git push origin --delete feature/x", Box::new(|g| { g.delete_branch_remote("feature/x").ok(); })),
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
        ("git ls-remote --tags origin v2.5.0", Box::new(|g| { g.remote_tag_exists("v2.5.0").ok(); })),
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

#[test]
fn find_stash_by_message_returns_the_matching_ref() {
    // The stash-pop path never pops blind: the ref comes from matching the
    // gflow-written message in `git stash list`.
    let runner = MockCommandRunner::ok(
        "stash@{0} On develop: WIP unrelated\n\
         stash@{1} On release/1.1.0: gflow-finish:release/1.1.0:123\n");

    assert_eq!(
        git(&runner).find_stash_by_message("gflow-finish:release/1.1.0:123").unwrap(),
        Some("stash@{1}".to_string())
    );
}

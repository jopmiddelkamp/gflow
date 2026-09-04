pub mod branch;

use std::path::{Path, PathBuf};
use std::process::Command;

pub type Result<T> = std::result::Result<T, String>;

fn utf8_path(path: &Path) -> Result<&str> {
    path.to_str().ok_or_else(|| format!("Path is not valid UTF-8: {}", path.display()))
}

pub trait Git {
    fn current_branch(&self) -> Result<String>;
    fn fetch(&self) -> Result<()>;
    fn checkout(&self, branch: &str) -> Result<()>;
    fn create_branch(&self, branch: &str, from: &str) -> Result<()>;
    fn create_branch_no_checkout(&self, branch: &str, from: &str) -> Result<()>;
    fn push(&self, branch: &str) -> Result<()>;
    fn push_tag(&self, tag: &str) -> Result<()>;
    fn create_tag(&self, tag: &str, message: &str) -> Result<()>;
    fn merge(&self, branch: &str, message: &str) -> Result<()>;
    fn ff_merge(&self, branch: &str) -> Result<()>;
    fn list_tags(&self) -> Result<Vec<String>>;
    /// Contract: returns a sorted, deduplicated list — consumers pick
    /// candidates by position (`.first()`), so order must be deterministic.
    fn list_branches_matching(&self, pattern: &str) -> Result<Vec<String>>;
    fn is_working_tree_clean(&self) -> Result<bool>;
    fn delete_branch_local(&self, branch: &str) -> Result<()>;
    fn delete_branch_remote(&self, branch: &str) -> Result<()>;
    fn tags_on_branch(&self, branch: &str) -> Result<Vec<String>>;
    fn list_remote_branches(&self) -> Result<Vec<String>>;
    fn merge_base(&self, a: &str, b: &str) -> Result<String>;
    fn rev_list_count(&self, from: &str, to: &str) -> Result<u32>;
    /// Number of parents of `sha` (2+ = a merge commit, 1 = a plain commit,
    /// 0 = a root commit). An unknown commit is an error, never a zero.
    fn commit_parent_count(&self, sha: &str) -> Result<u32>;
    fn commit_messages(&self, from: &str, to: &str) -> Result<Vec<String>>;

    // Idempotency primitives
    fn is_ancestor(&self, ancestor: &str, descendant: &str) -> Result<bool>;
    fn tag_exists(&self, tag: &str) -> Result<bool>;
    fn local_branch_exists(&self, branch: &str) -> Result<bool>;
    fn remote_branch_exists(&self, branch: &str) -> Result<bool>;
    fn remote_tag_exists(&self, tag: &str) -> Result<bool>;
    fn is_pushed(&self, branch: &str) -> Result<bool>;
    fn is_mid_merge(&self) -> Result<bool>;
    fn has_unmerged_paths(&self) -> Result<bool>;
    fn git_dir(&self) -> Result<PathBuf>;

    /// URL of the `origin` remote (`git remote get-url origin`). Errors when the
    /// remote does not exist; callers decide how to handle a missing remote.
    fn remote_url(&self) -> Result<String>;

    // Worktree / config primitives
    /// Read a git config value (`git config --get <key>`). Returns `None` when unset.
    fn get_config(&self, key: &str) -> Result<Option<String>>;
    /// Write a git config value. `global` selects `--global` (user) vs local (repo) scope.
    fn set_config(&self, key: &str, value: &str, global: bool) -> Result<()>;
    /// Remove a git config value. A key that is already unset is treated as success.
    fn unset_config(&self, key: &str, global: bool) -> Result<()>;
    /// Absolute path to the MAIN working tree's root. Stable even when run from
    /// inside a linked worktree (`rev-parse --show-toplevel` would return the
    /// worktree's own directory there, compounding worktree folder names).
    fn repo_root(&self) -> Result<PathBuf>;
    /// Absolute path to the root of the working tree the command is running in —
    /// the linked worktree when standing in one, the main tree otherwise. Repo
    /// *content* (`.gflow/config`, the version script, PR templates) must be read
    /// from here: a linked worktree can have a different branch checked out than
    /// the main tree, and reading the main tree's copy would apply another
    /// branch's policy. Contrast `repo_root`, which is deliberately the MAIN
    /// tree's root and stays correct for worktree bookkeeping.
    fn worktree_root(&self) -> Result<PathBuf>;
    /// Add a worktree at `path` checked out to the (already existing) `branch`.
    fn add_worktree(&self, path: &Path, branch: &str) -> Result<()>;
    /// Root of the working tree (main or linked) that has `branch` checked out,
    /// `None` when no tree holds it. Git refuses to check out a branch that is
    /// held by another tree, so flows merge into such a branch in place instead.
    fn worktree_of(&self, branch: &str) -> Result<Option<PathBuf>>;
    /// `is_working_tree_clean` / `ff_merge` / `merge`, run in the working tree
    /// at `path` (`git -C`) instead of the current one.
    fn is_working_tree_clean_at(&self, path: &Path) -> Result<bool>;
    fn ff_merge_at(&self, path: &Path, branch: &str) -> Result<()>;
    fn merge_at(&self, path: &Path, branch: &str, message: &str) -> Result<()>;
    /// Whether the current checkout is a linked worktree rather than the main
    /// working tree.
    fn is_linked_worktree(&self) -> Result<bool>;
    /// Remove the linked worktree we are standing in and return its path.
    /// Must be the LAST git operation of a flow: the process working directory
    /// no longer exists afterwards, so any further subprocess would fail.
    fn remove_current_worktree(&self) -> Result<PathBuf>;
    /// SHA of the current HEAD commit.
    fn head_sha(&self) -> Result<String>;
    /// Detach HEAD from the current branch (frees the branch for deletion while
    /// this worktree still exists).
    fn detach_head(&self) -> Result<()>;

    // Stash by message (safer than blind pop)
    fn stash_push_with_message(&self, msg: &str) -> Result<()>;
    fn find_stash_by_message(&self, msg: &str) -> Result<Option<String>>;
    fn stash_pop_ref(&self, stash_ref: &str) -> Result<()>;

    // Version commits and merge-commit tagging
    fn stage_all(&self) -> Result<()>;
    fn commit(&self, message: &str) -> Result<()>;
    /// Tags `sha` (not HEAD) — a separate method from `create_tag` because a
    /// landing PR's merge commit is created by the hosting platform, not by a
    /// local `git merge`.
    fn create_tag_at(&self, tag: &str, message: &str, sha: &str) -> Result<()>;
    /// Resolves an annotated tag to the commit it points at (`^{commit}`).
    /// Plain `rev-parse <tag>` on an annotated tag returns the tag object's own
    /// SHA, not the commit's.
    fn tag_commit_sha(&self, tag: &str) -> Result<String>;
    /// SHA of a branch's tip — never HEAD, which is only the source branch when
    /// the caller happens to be standing on it.
    fn branch_sha(&self, branch: &str) -> Result<String>;
}

/// One finished `git` invocation, in a form tests can construct. `std::process::
/// Output` cannot be built portably (`ExitStatus` has no cross-platform
/// constructor), and exit codes are load-bearing here — `git config --get`
/// exits 1 for "not set", `--unset` exits 5 for "already unset" — so the seam
/// carries the raw code rather than a success/failure boolean.
pub struct CliOutput {
    /// `None` when the process was terminated by a signal.
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

/// Port for spawning `git`. `GitCli` owns the exit-code semantics and output
/// parsing; this trait owns only the process spawn, keeping "no subprocess calls
/// outside adapter impls" (SKILL.md principle 1) true at a single point.
pub trait CommandRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<CliOutput>;
}

/// The real runner: spawns `git` as a child process.
pub struct SystemRunner;

impl CommandRunner for SystemRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<CliOutput> {
        let output = Command::new(program).args(args).output()
            .map_err(|e| format!("Failed to run {program}: {e}"))?;
        Ok(CliOutput {
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }
}

pub struct GitCli<'a> {
    runner: &'a dyn CommandRunner,
}

impl<'a> GitCli<'a> {
    pub fn new(runner: &'a dyn CommandRunner) -> Self {
        Self { runner }
    }

    fn output(&self, args: &[&str]) -> Result<CliOutput> {
        self.runner.run("git", args)
    }

    fn run(&self, args: &[&str]) -> Result<String> {
        let output = self.output(args)?;
        if output.code == Some(0) {
            Ok(output.stdout.trim().to_string())
        } else {
            Err(format!("git {} failed: {}", args.join(" "), output.stderr.trim()))
        }
    }

    fn run_lines(&self, args: &[&str]) -> Result<Vec<String>> {
        let output = self.run(args)?;
        Ok(output.lines().map(|s| s.to_string()).filter(|s| !s.is_empty()).collect())
    }

    /// Run a check command that uses exit 0/1 as a true/false result
    /// (e.g., `merge-base --is-ancestor`, `show-ref --verify`).
    /// Exit codes other than 0 or 1 are treated as errors.
    fn run_check(&self, args: &[&str]) -> Result<bool> {
        let output = self.output(args)?;
        match output.code {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(unexpected_exit(args, &output)),
        }
    }

    /// Resolve a ref name to its SHA. Only needed internally (`is_pushed`
    /// compares local vs remote SHAs), so not part of the `Git` port.
    fn rev_parse(&self, refname: &str) -> Result<String> {
        self.run(&["rev-parse", refname])
    }

    /// Run a `git config --get`-style command that uses exit 1 to mean "key not set".
    /// Returns `Ok(None)` on exit 1, `Ok(Some(value))` on exit 0, and an error otherwise.
    fn run_config(&self, args: &[&str]) -> Result<Option<String>> {
        let output = self.output(args)?;
        match output.code {
            Some(0) => Ok(Some(output.stdout.trim().to_string())),
            Some(1) => Ok(None),
            _ => Err(unexpected_exit(args, &output)),
        }
    }
}

fn unexpected_exit(args: &[&str], output: &CliOutput) -> String {
    match output.code {
        Some(code) => format!("git {} failed (exit {code}): {}", args.join(" "), output.stderr.trim()),
        None => format!("git {} terminated by signal", args.join(" ")),
    }
}

impl Git for GitCli<'_> {
    fn current_branch(&self) -> Result<String> { self.run(&["rev-parse", "--abbrev-ref", "HEAD"]) }
    fn fetch(&self) -> Result<()> { self.run(&["fetch", "--all", "--prune"]).map(|_| ()) }
    fn checkout(&self, branch: &str) -> Result<()> { self.run(&["checkout", branch]).map(|_| ()) }
    fn create_branch(&self, branch: &str, from: &str) -> Result<()> { self.run(&["checkout", "-b", branch, from]).map(|_| ()) }
    fn create_branch_no_checkout(&self, branch: &str, from: &str) -> Result<()> { self.run(&["branch", branch, from]).map(|_| ()) }
    fn push(&self, branch: &str) -> Result<()> { self.run(&["push", "-u", "origin", branch]).map(|_| ()) }
    fn push_tag(&self, tag: &str) -> Result<()> { self.run(&["push", "origin", tag]).map(|_| ()) }
    fn create_tag(&self, tag: &str, message: &str) -> Result<()> { self.run(&["tag", "-a", tag, "-m", message]).map(|_| ()) }
    fn merge(&self, branch: &str, message: &str) -> Result<()> { self.run(&["merge", branch, "--no-ff", "-m", message]).map(|_| ()) }
    fn ff_merge(&self, branch: &str) -> Result<()> { self.run(&["merge", branch, "--ff-only"]).map(|_| ()) }
    fn list_tags(&self) -> Result<Vec<String>> {
        self.run_lines(&["tag", "--list"])
    }
    fn list_branches_matching(&self, pattern: &str) -> Result<Vec<String>> {
        let ref_pattern = format!("refs/remotes/origin/{pattern}");
        let local_pattern = format!("refs/heads/{pattern}");
        let lines = self.run_lines(&[
            "for-each-ref", "--format=%(refname:short)",
            &ref_pattern, &local_pattern,
        ])?;
        // Sorted + deduped per the trait contract.
        let mut branches: Vec<String> = lines
            .iter()
            .map(|s| s.trim_start_matches("origin/").to_string())
            .collect();
        branches.sort();
        branches.dedup();
        Ok(branches)
    }
    fn is_working_tree_clean(&self) -> Result<bool> {
        let output = self.run(&["status", "--porcelain"])?;
        Ok(output.is_empty())
    }
    fn delete_branch_local(&self, branch: &str) -> Result<()> { self.run(&["branch", "-D", branch]).map(|_| ()) }
    fn delete_branch_remote(&self, branch: &str) -> Result<()> { self.run(&["push", "origin", "--delete", branch]).map(|_| ()) }
    fn tags_on_branch(&self, branch: &str) -> Result<Vec<String>> {
        self.run_lines(&["tag", "--merged", branch])
    }
    fn list_remote_branches(&self) -> Result<Vec<String>> {
        let lines = self.run_lines(&["for-each-ref", "--format=%(refname:short)", "refs/remotes/origin/"])?;
        Ok(lines
            .iter()
            .map(|s| s.trim_start_matches("origin/").to_string())
            .filter(|s| s != "HEAD")
            .collect())
    }
    fn merge_base(&self, a: &str, b: &str) -> Result<String> {
        self.run(&["merge-base", a, b])
    }
    fn rev_list_count(&self, from: &str, to: &str) -> Result<u32> {
        let range = format!("{from}..{to}");
        let output = self.run(&["rev-list", "--count", &range])?;
        output.parse::<u32>().map_err(|e| format!("Failed to parse rev-list count: {e}"))
    }
    fn commit_parent_count(&self, sha: &str) -> Result<u32> {
        // Output: the commit followed by its parents, whitespace-separated.
        let output = self.run(&["rev-list", "--parents", "-n", "1", sha])?;
        let count = output.split_whitespace().count();
        if count == 0 {
            return Err(format!("Could not read parents of commit '{sha}'. Run 'git fetch' and retry."));
        }
        Ok((count - 1) as u32)
    }
    fn commit_messages(&self, from: &str, to: &str) -> Result<Vec<String>> {
        let range = format!("{from}..{to}");
        // Use NUL byte as separator — guaranteed not to appear in commit messages
        let output = self.run(&["log", &range, "--format=%B%x00"])?;
        Ok(output.split('\0')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect())
    }

    fn is_ancestor(&self, ancestor: &str, descendant: &str) -> Result<bool> {
        self.run_check(&["merge-base", "--is-ancestor", ancestor, descendant])
    }
    fn tag_exists(&self, tag: &str) -> Result<bool> {
        self.run_check(&["show-ref", "--verify", "--quiet", &format!("refs/tags/{tag}")])
    }
    fn local_branch_exists(&self, branch: &str) -> Result<bool> {
        self.run_check(&["show-ref", "--verify", "--quiet", &format!("refs/heads/{branch}")])
    }
    fn remote_branch_exists(&self, branch: &str) -> Result<bool> {
        self.run_check(&["show-ref", "--verify", "--quiet", &format!("refs/remotes/origin/{branch}")])
    }
    fn remote_tag_exists(&self, tag: &str) -> Result<bool> {
        let output = self.run(&["ls-remote", "--tags", "origin", tag])?;
        Ok(!output.trim().is_empty())
    }
    fn is_pushed(&self, branch: &str) -> Result<bool> {
        let local = self.rev_parse(branch)?;
        let remote_ref = format!("refs/remotes/origin/{branch}");
        let remote = match self.rev_parse(&remote_ref) {
            Ok(s) => s,
            Err(_) => return Ok(false),
        };
        Ok(local == remote)
    }
    fn is_mid_merge(&self) -> Result<bool> {
        let dir = self.git_dir()?;
        Ok(dir.join("MERGE_HEAD").exists()
            || dir.join("CHERRY_PICK_HEAD").exists()
            || dir.join("REVERT_HEAD").exists()
            || dir.join("rebase-merge").exists()
            || dir.join("rebase-apply").exists())
    }
    fn has_unmerged_paths(&self) -> Result<bool> {
        let output = self.run(&["status", "--porcelain"])?;
        for line in output.lines() {
            // Porcelain conflict markers: U? / ?U / AA / DD / AU / UA / DU / UD
            let bytes = line.as_bytes();
            if bytes.len() < 2 { continue; }
            let (x, y) = (bytes[0] as char, bytes[1] as char);
            if x == 'U' || y == 'U' || (x == 'A' && y == 'A') || (x == 'D' && y == 'D') {
                return Ok(true);
            }
        }
        Ok(false)
    }
    fn git_dir(&self) -> Result<PathBuf> {
        let s = self.run(&["rev-parse", "--git-dir"])?;
        Ok(PathBuf::from(s))
    }

    fn remote_url(&self) -> Result<String> {
        self.run(&["remote", "get-url", "origin"])
    }

    fn get_config(&self, key: &str) -> Result<Option<String>> {
        self.run_config(&["config", "--get", key])
    }
    fn set_config(&self, key: &str, value: &str, global: bool) -> Result<()> {
        let mut args = vec!["config"];
        if global { args.push("--global"); }
        args.push(key);
        args.push(value);
        self.run(&args).map(|_| ())
    }
    fn unset_config(&self, key: &str, global: bool) -> Result<()> {
        let mut args = vec!["config"];
        if global { args.push("--global"); }
        args.push("--unset");
        args.push(key);
        let output = self.output(&args)?;
        match output.code {
            // 0 = removed; 5 = key was not set (already at default) — both fine.
            Some(0) | Some(5) => Ok(()),
            _ => Err(unexpected_exit(&args, &output)),
        }
    }
    fn repo_root(&self) -> Result<PathBuf> {
        // `git worktree list` always lists the main working tree first, so this
        // resolves the same root regardless of which worktree we run from.
        let output = self.run(&["worktree", "list", "--porcelain"])?;
        output
            .lines()
            .find_map(|l| l.strip_prefix("worktree "))
            .map(PathBuf::from)
            .ok_or_else(|| "Could not determine the main working tree from 'git worktree list'.".to_string())
    }
    fn worktree_root(&self) -> Result<PathBuf> {
        self.run(&["rev-parse", "--show-toplevel"]).map(PathBuf::from)
    }
    fn add_worktree(&self, path: &Path, branch: &str) -> Result<()> {
        let path_str = path.to_str().ok_or("Worktree path is not valid UTF-8")?;
        self.run(&["worktree", "add", path_str, branch]).map(|_| ())
    }
    fn worktree_of(&self, branch: &str) -> Result<Option<PathBuf>> {
        let output = self.run(&["worktree", "list", "--porcelain"])?;
        let wanted = format!("branch refs/heads/{branch}");
        Ok(output
            .split("\n\n")
            .find(|entry| entry.lines().any(|l| l == wanted))
            .and_then(|entry| entry.lines().find_map(|l| l.strip_prefix("worktree ")))
            .map(PathBuf::from))
    }
    fn is_working_tree_clean_at(&self, path: &Path) -> Result<bool> {
        let output = self.run(&["-C", utf8_path(path)?, "status", "--porcelain"])?;
        Ok(output.is_empty())
    }
    fn ff_merge_at(&self, path: &Path, branch: &str) -> Result<()> {
        self.run(&["-C", utf8_path(path)?, "merge", branch, "--ff-only"]).map(|_| ())
    }
    fn merge_at(&self, path: &Path, branch: &str, message: &str) -> Result<()> {
        self.run(&["-C", utf8_path(path)?, "merge", branch, "--no-ff", "-m", message]).map(|_| ())
    }
    fn is_linked_worktree(&self) -> Result<bool> {
        // One invocation for both paths so the two are in a consistent form:
        // they are equal for the main working tree, and differ (<common>/worktrees/<name>
        // vs <common>) for a linked one.
        let output = self.run(&["rev-parse", "--git-dir", "--git-common-dir"])?;
        let mut lines = output.lines();
        match (lines.next(), lines.next()) {
            (Some(git_dir), Some(common_dir)) => Ok(git_dir != common_dir),
            _ => Err(format!("Unexpected 'git rev-parse --git-dir --git-common-dir' output: '{output}'")),
        }
    }
    fn remove_current_worktree(&self) -> Result<PathBuf> {
        let own_root = self.run(&["rev-parse", "--show-toplevel"])?;
        // git refuses to remove the worktree it runs in, so run from the main
        // working tree via -C. `--force` is safe here: the finish preflight
        // already rejected dirty trees, so at most ignored files are deleted.
        let main_root = self.repo_root()?;
        let main_root = main_root.to_str().ok_or("Main working tree path is not valid UTF-8")?;
        self.run(&["-C", main_root, "worktree", "remove", "--force", &own_root])?;
        Ok(PathBuf::from(own_root))
    }
    fn head_sha(&self) -> Result<String> {
        self.rev_parse("HEAD")
    }
    fn detach_head(&self) -> Result<()> {
        self.run(&["checkout", "--detach"]).map(|_| ())
    }

    fn stash_push_with_message(&self, msg: &str) -> Result<()> {
        self.run(&["stash", "push", "-u", "-m", msg]).map(|_| ())
    }
    fn find_stash_by_message(&self, msg: &str) -> Result<Option<String>> {
        let output = self.run(&["stash", "list", "--format=%gd %s"])?;
        for line in output.lines() {
            if let Some((ref_, rest)) = line.split_once(' ') {
                if rest.contains(msg) {
                    return Ok(Some(ref_.to_string()));
                }
            }
        }
        Ok(None)
    }
    fn stash_pop_ref(&self, stash_ref: &str) -> Result<()> {
        self.run(&["stash", "pop", stash_ref]).map(|_| ())
    }

    fn stage_all(&self) -> Result<()> { self.run(&["add", "-A"]).map(|_| ()) }
    fn commit(&self, message: &str) -> Result<()> { self.run(&["commit", "-m", message]).map(|_| ()) }
    fn create_tag_at(&self, tag: &str, message: &str, sha: &str) -> Result<()> {
        self.run(&["tag", "-a", tag, "-m", message, sha]).map(|_| ())
    }
    fn tag_commit_sha(&self, tag: &str) -> Result<String> {
        self.run(&["rev-parse", &format!("{tag}^{{commit}}")])
    }
    fn branch_sha(&self, branch: &str) -> Result<String> {
        self.run(&["rev-parse", &format!("refs/heads/{branch}")])
    }
}

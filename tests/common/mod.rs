// Shared mocks for the integration-test suites. Each test crate compiles its
// own copy of this module and rarely uses every mock, so item-level dead_code
// warnings here are pure noise — silenced module-wide.
#![allow(dead_code)]

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use gflow::action::validate_branch_name;
use gflow::editor::Editor;
use gflow::git::{CliOutput, CommandRunner, Git};
use gflow::hosting::{CliRunner, HostingPlatform, PrBody};
use gflow::prompt::Prompter;
use gflow::version_script::VersionScript;

const MERGE_BASE: &str = "abc123";
const REMOVED_WORKTREE_PATH: &str = "/repos/beans-gitflow-feature-x";

pub struct MockGit {
    pub calls: RefCell<Vec<String>>,
    pub current_branch: String,
    pub tags: Vec<String>,
    pub tags_on_branch: Vec<String>,
    pub branches_matching: Vec<String>,
    /// Per-pattern results for `list_branches_matching`; misses fall back to `branches_matching`.
    pub branches_matching_by: HashMap<String, Vec<String>>,
    pub remote_branches: Vec<String>,
    /// Per-(a, b) merge bases; falls back to `MERGE_BASE` when absent.
    pub merge_bases: HashMap<(String, String), String>,
    pub rev_list_count_result: u32,
    /// Per-(from, to) counts; falls back to `rev_list_count_result` when absent.
    pub rev_list_counts: HashMap<(String, String), u32>,
    /// Parent count per commit sha (`commit_parent_count`). A sha not in the map
    /// is an error, matching git's behavior for an unknown commit — tests that
    /// reach a completion-type check must say how the PR was completed.
    pub parent_counts: HashMap<String, u32>,
    pub commit_messages: Vec<String>,
    /// Refs (the `to` arg) that should fail with an error. Used to simulate missing branches.
    pub fail_commit_messages_for: Vec<String>,
    /// Branches (the `b` arg) whose merge base cannot be computed — an unrelated
    /// history. Parent detection must skip them rather than fail.
    pub fail_merge_base_for: Vec<String>,
    /// Refs (the `to` arg) whose commit count cannot be computed.
    pub fail_rev_list_count_for: Vec<String>,
    /// 1-indexed merge call to fail (simulates a merge conflict). None = never fail.
    pub fail_nth_merge: Option<u32>,
    merge_call_count: RefCell<u32>,
    /// When set, every `ff_merge` fails with this message. The text matters: flows
    /// distinguish "no upstream yet" (`not something we can merge`, swallowed) from
    /// a real failure (surfaced).
    pub ff_merge_error: Option<String>,
    /// When set, `create_branch` / `create_branch_no_checkout` fail with this
    /// message. The text matters: flows rewrite git's "not a commit" into their
    /// own guidance and pass everything else through.
    pub create_branch_error: Option<String>,
    /// `stash_pop_ref` fails (simulates a conflicting pop).
    pub fail_stash_pop: bool,
    /// `find_stash_by_message` fails (simulates an unreadable stash list).
    pub fail_find_stash: bool,

    // Idempotency state — branches/tags considered "already done".
    /// Pairs of (ancestor, descendant) where ancestor is treated as merged into descendant.
    pub ancestors: HashSet<(String, String)>,
    /// Postcondition of `create_branch`/`create_branch_no_checkout`: the new
    /// branch points at `from`, so `from` is an ancestor of it — and stays one,
    /// since branches are only appended to. Modeled here (LSP: the mock is an
    /// implementation too) so flows can trust the contract instead of tests
    /// hand-feeding what git guarantees.
    created_ancestors: RefCell<HashSet<(String, String)>>,
    /// Existing local tags (idempotent skipping for tag creation).
    pub existing_tags: HashSet<String>,
    /// Existing remote tags.
    pub existing_remote_tags: HashSet<String>,
    /// Existing local branches.
    pub existing_local_branches: HashSet<String>,
    /// Existing remote branches.
    pub existing_remote_branches: HashSet<String>,
    /// Branches whose local SHA matches origin's (treated as pushed).
    pub pushed_branches: HashSet<String>,
    pub mid_merge: bool,
    pub unmerged_paths: bool,
    /// What `is_working_tree_clean` reports (defaults to clean).
    pub working_tree_clean: bool,
    pub git_dir: PathBuf,
    /// Stash messages currently in the stash list (most recent first).
    pub stashes: RefCell<Vec<String>>,
    /// git config values returned by `get_config` (key -> value).
    pub config: HashMap<String, String>,
    /// URL returned by `remote_url`.
    pub remote_url: String,
    /// `remote_url` fails (a repo with no `origin` configured).
    pub fail_remote_url: bool,
    /// Value returned by `repo_root`.
    pub repo_root: PathBuf,
    /// Value returned by `worktree_root`.
    pub worktree_root: PathBuf,
    /// SHA returned by `head_sha`.
    pub head_sha: String,
    /// Whether the current checkout is a linked worktree.
    pub linked_worktree: bool,
    /// Branch -> root of the working tree that has it checked out (`worktree_of`).
    pub worktrees: HashMap<String, PathBuf>,
    /// Commit SHA each annotated tag resolves to via `tag_commit_sha`. A tag
    /// missing here fails, modeling "tag doesn't exist" (flows only call this
    /// after `tag_exists`).
    pub tag_commits: HashMap<String, String>,
    /// SHA each branch's tip resolves to via `branch_sha`. A branch missing
    /// here falls back to `head_sha`.
    pub branch_shas: HashMap<String, String>,
    /// Scripted `is_working_tree_clean` answers, consumed front-first. Empty
    /// (the default) falls back to `working_tree_clean`.
    pub working_tree_clean_seq: RefCell<VecDeque<bool>>,
    _git_dir_guard: Option<TempDir>,
}

impl MockGit {
    pub fn new() -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            current_branch: "develop".to_string(),
            tags: Vec::new(),
            tags_on_branch: Vec::new(),
            branches_matching: Vec::new(),
            branches_matching_by: HashMap::new(),
            remote_branches: Vec::new(),
            merge_bases: HashMap::new(),
            rev_list_count_result: 0,
            rev_list_counts: HashMap::new(),
            parent_counts: HashMap::new(),
            commit_messages: Vec::new(),
            fail_commit_messages_for: Vec::new(),
            fail_merge_base_for: Vec::new(),
            fail_rev_list_count_for: Vec::new(),
            fail_nth_merge: None,
            merge_call_count: RefCell::new(0),
            ff_merge_error: None,
            create_branch_error: None,
            fail_stash_pop: false,
            fail_find_stash: false,
            ancestors: HashSet::new(),
            created_ancestors: RefCell::new(HashSet::new()),
            existing_tags: HashSet::new(),
            existing_remote_tags: HashSet::new(),
            existing_local_branches: HashSet::new(),
            existing_remote_branches: HashSet::new(),
            pushed_branches: HashSet::new(),
            mid_merge: false,
            unmerged_paths: false,
            working_tree_clean: true,
            git_dir: PathBuf::from(".git"),
            stashes: RefCell::new(Vec::new()),
            config: HashMap::new(),
            remote_url: "https://github.com/acme/repo.git".to_string(),
            fail_remote_url: false,
            repo_root: PathBuf::from("/repos/beans-gitflow"),
            worktree_root: PathBuf::from("/repos/beans-gitflow"),
            head_sha: "headsha".to_string(),
            linked_worktree: false,
            worktrees: HashMap::new(),
            tag_commits: HashMap::new(),
            branch_shas: HashMap::new(),
            working_tree_clean_seq: RefCell::new(VecDeque::new()),
            _git_dir_guard: None,
        }
    }

    /// A mock whose `git_dir()` is a real temp directory, removed on drop.
    pub fn with_tmp_git_dir(prefix: &str) -> Self {
        let dir = tmp_dir(prefix);
        Self { git_dir: dir.to_path_buf(), _git_dir_guard: Some(dir), ..Self::new() }
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }

    fn create_branch_result(&self) -> Result<(), String> {
        match &self.create_branch_error {
            Some(e) => Err(e.clone()),
            None => Ok(()),
        }
    }
}

impl Git for MockGit {
    fn current_branch(&self) -> Result<String, String> {
        self.calls.borrow_mut().push("current_branch".to_string());
        Ok(self.current_branch.clone())
    }

    fn fetch(&self) -> Result<(), String> {
        self.calls.borrow_mut().push("fetch".to_string());
        Ok(())
    }

    fn checkout(&self, branch: &str) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("checkout:{branch}"));
        Ok(())
    }

    fn create_branch(&self, branch: &str, from: &str) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("create_branch:{branch}:{from}"));
        self.create_branch_result()?;
        self.created_ancestors.borrow_mut().insert((from.to_string(), branch.to_string()));
        Ok(())
    }

    fn create_branch_no_checkout(&self, branch: &str, from: &str) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("create_branch_no_checkout:{branch}:{from}"));
        self.create_branch_result()?;
        self.created_ancestors.borrow_mut().insert((from.to_string(), branch.to_string()));
        Ok(())
    }

    fn push(&self, branch: &str) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("push:{branch}"));
        Ok(())
    }

    fn push_tag(&self, tag: &str) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("push_tag:{tag}"));
        Ok(())
    }

    fn create_tag(&self, tag: &str, message: &str) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("create_tag:{tag}:{message}"));
        Ok(())
    }

    fn merge(&self, branch: &str, message: &str) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("merge:{branch}:{message}"));
        let mut count = self.merge_call_count.borrow_mut();
        *count += 1;
        if Some(*count) == self.fail_nth_merge {
            return Err(format!("CONFLICT: merge of {branch} failed"));
        }
        Ok(())
    }

    fn ff_merge(&self, branch: &str) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("ff_merge:{branch}"));
        match &self.ff_merge_error {
            Some(e) => Err(e.clone()),
            None => Ok(()),
        }
    }

    fn list_tags(&self) -> Result<Vec<String>, String> {
        self.calls.borrow_mut().push("list_tags".to_string());
        Ok(self.tags.clone())
    }

    fn list_branches_matching(&self, pattern: &str) -> Result<Vec<String>, String> {
        self.calls.borrow_mut().push(format!("list_branches_matching:{pattern}"));
        if let Some(matches) = self.branches_matching_by.get(pattern) {
            return Ok(matches.clone());
        }
        // git's ref patterns: `release/*` can never match `release-fix/…`.
        let prefix = pattern.strip_suffix('*').unwrap_or(pattern);
        Ok(self.branches_matching.iter().filter(|b| b.starts_with(prefix)).cloned().collect())
    }

    fn is_working_tree_clean(&self) -> Result<bool, String> {
        self.calls.borrow_mut().push("is_working_tree_clean".to_string());
        match self.working_tree_clean_seq.borrow_mut().pop_front() {
            Some(clean) => Ok(clean),
            None => Ok(self.working_tree_clean),
        }
    }

    fn delete_branch_local(&self, branch: &str) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("delete_branch_local:{branch}"));
        Ok(())
    }

    fn delete_branch_remote(&self, branch: &str) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("delete_branch_remote:{branch}"));
        Ok(())
    }

    fn tags_on_branch(&self, branch: &str) -> Result<Vec<String>, String> {
        self.calls.borrow_mut().push(format!("tags_on_branch:{branch}"));
        Ok(self.tags_on_branch.clone())
    }

    fn list_remote_branches(&self) -> Result<Vec<String>, String> {
        self.calls.borrow_mut().push("list_remote_branches".to_string());
        Ok(self.remote_branches.clone())
    }

    fn merge_base(&self, a: &str, b: &str) -> Result<String, String> {
        self.calls.borrow_mut().push(format!("merge_base:{a}:{b}"));
        if self.fail_merge_base_for.iter().any(|r| r == b) {
            return Err(format!("no merge base between {a} and {b}"));
        }
        Ok(self.merge_bases
            .get(&(a.to_string(), b.to_string()))
            .cloned()
            .unwrap_or_else(|| MERGE_BASE.to_string()))
    }

    fn rev_list_count(&self, from: &str, to: &str) -> Result<u32, String> {
        self.calls.borrow_mut().push(format!("rev_list_count:{from}:{to}"));
        if self.fail_rev_list_count_for.iter().any(|r| r == to) {
            return Err(format!("bad revision: {to}"));
        }
        Ok(*self.rev_list_counts
            .get(&(from.to_string(), to.to_string()))
            .unwrap_or(&self.rev_list_count_result))
    }
    fn commit_parent_count(&self, sha: &str) -> Result<u32, String> {
        self.calls.borrow_mut().push(format!("commit_parent_count:{sha}"));
        self.parent_counts
            .get(sha)
            .copied()
            .ok_or(format!("Could not read parents of commit '{sha}'. Run 'git fetch' and retry."))
    }

    fn commit_messages(&self, from: &str, to: &str) -> Result<Vec<String>, String> {
        self.calls.borrow_mut().push(format!("commit_messages:{from}:{to}"));
        if self.fail_commit_messages_for.iter().any(|r| r == to) {
            return Err(format!("ref not found: {to}"));
        }
        Ok(self.commit_messages.clone())
    }

    fn is_ancestor(&self, ancestor: &str, descendant: &str) -> Result<bool, String> {
        self.calls.borrow_mut().push(format!("is_ancestor:{ancestor}:{descendant}"));
        let key = (ancestor.to_string(), descendant.to_string());
        Ok(self.ancestors.contains(&key) || self.created_ancestors.borrow().contains(&key))
    }

    fn tag_exists(&self, tag: &str) -> Result<bool, String> {
        self.calls.borrow_mut().push(format!("tag_exists:{tag}"));
        Ok(self.existing_tags.contains(tag))
    }

    fn local_branch_exists(&self, branch: &str) -> Result<bool, String> {
        self.calls.borrow_mut().push(format!("local_branch_exists:{branch}"));
        Ok(self.existing_local_branches.contains(branch))
    }

    fn remote_branch_exists(&self, branch: &str) -> Result<bool, String> {
        self.calls.borrow_mut().push(format!("remote_branch_exists:{branch}"));
        Ok(self.existing_remote_branches.contains(branch))
    }

    fn remote_tag_exists(&self, tag: &str) -> Result<bool, String> {
        self.calls.borrow_mut().push(format!("remote_tag_exists:{tag}"));
        Ok(self.existing_remote_tags.contains(tag))
    }

    fn is_pushed(&self, branch: &str) -> Result<bool, String> {
        self.calls.borrow_mut().push(format!("is_pushed:{branch}"));
        Ok(self.pushed_branches.contains(branch))
    }

    fn is_mid_merge(&self) -> Result<bool, String> {
        self.calls.borrow_mut().push("is_mid_merge".to_string());
        Ok(self.mid_merge)
    }

    fn has_unmerged_paths(&self) -> Result<bool, String> {
        self.calls.borrow_mut().push("has_unmerged_paths".to_string());
        Ok(self.unmerged_paths)
    }

    fn git_dir(&self) -> Result<PathBuf, String> {
        self.calls.borrow_mut().push("git_dir".to_string());
        Ok(self.git_dir.clone())
    }

    fn remote_url(&self) -> Result<String, String> {
        self.calls.borrow_mut().push("remote_url".to_string());
        if self.fail_remote_url {
            return Err("No such remote 'origin'".to_string());
        }
        Ok(self.remote_url.clone())
    }

    fn get_config(&self, key: &str) -> Result<Option<String>, String> {
        self.calls.borrow_mut().push(format!("get_config:{key}"));
        Ok(self.config.get(key).cloned())
    }

    fn set_config(&self, key: &str, value: &str, global: bool) -> Result<(), String> {
        let scope = if global { "global" } else { "local" };
        self.calls.borrow_mut().push(format!("set_config:{scope}:{key}:{value}"));
        Ok(())
    }

    fn unset_config(&self, key: &str, global: bool) -> Result<(), String> {
        let scope = if global { "global" } else { "local" };
        self.calls.borrow_mut().push(format!("unset_config:{scope}:{key}"));
        Ok(())
    }

    fn repo_root(&self) -> Result<PathBuf, String> {
        self.calls.borrow_mut().push("repo_root".to_string());
        Ok(self.repo_root.clone())
    }

    fn worktree_root(&self) -> Result<PathBuf, String> {
        self.calls.borrow_mut().push("worktree_root".to_string());
        Ok(self.worktree_root.clone())
    }

    fn add_worktree(&self, path: &Path, branch: &str) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("add_worktree:{}:{branch}", path.display()));
        Ok(())
    }

    fn stash_push_with_message(&self, msg: &str) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("stash_push_with_message:{msg}"));
        self.stashes.borrow_mut().insert(0, msg.to_string());
        Ok(())
    }

    fn find_stash_by_message(&self, msg: &str) -> Result<Option<String>, String> {
        self.calls.borrow_mut().push(format!("find_stash_by_message:{msg}"));
        if self.fail_find_stash {
            return Err("could not read the stash list".to_string());
        }
        let stashes = self.stashes.borrow();
        for (i, m) in stashes.iter().enumerate() {
            if m == msg {
                return Ok(Some(format!("stash@{{{i}}}")));
            }
        }
        Ok(None)
    }

    fn stash_pop_ref(&self, stash_ref: &str) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("stash_pop_ref:{stash_ref}"));
        if self.fail_stash_pop {
            return Err("conflict while popping".to_string());
        }
        Ok(())
    }

    fn worktree_of(&self, branch: &str) -> Result<Option<PathBuf>, String> {
        self.calls.borrow_mut().push(format!("worktree_of:{branch}"));
        Ok(self.worktrees.get(branch).cloned())
    }
    fn is_working_tree_clean_at(&self, path: &Path) -> Result<bool, String> {
        self.calls.borrow_mut().push(format!("is_working_tree_clean_at:{}", path.display()));
        Ok(self.working_tree_clean)
    }
    fn ff_merge_at(&self, path: &Path, branch: &str) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("ff_merge_at:{}:{branch}", path.display()));
        Ok(())
    }
    fn merge_at(&self, path: &Path, branch: &str, message: &str) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("merge_at:{}:{branch}:{message}", path.display()));
        Ok(())
    }
    fn is_linked_worktree(&self) -> Result<bool, String> {
        self.calls.borrow_mut().push("is_linked_worktree".to_string());
        Ok(self.linked_worktree)
    }

    fn remove_current_worktree(&self) -> Result<PathBuf, String> {
        self.calls.borrow_mut().push("remove_current_worktree".to_string());
        Ok(PathBuf::from(REMOVED_WORKTREE_PATH))
    }

    fn head_sha(&self) -> Result<String, String> {
        self.calls.borrow_mut().push("head_sha".to_string());
        Ok(self.head_sha.clone())
    }

    fn detach_head(&self) -> Result<(), String> {
        self.calls.borrow_mut().push("detach_head".to_string());
        Ok(())
    }

    fn stage_all(&self) -> Result<(), String> {
        self.calls.borrow_mut().push("stage_all".to_string());
        Ok(())
    }

    fn commit(&self, message: &str) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("commit:{message}"));
        Ok(())
    }

    fn create_tag_at(&self, tag: &str, message: &str, sha: &str) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("create_tag_at:{tag}:{message}:{sha}"));
        Ok(())
    }

    fn tag_commit_sha(&self, tag: &str) -> Result<String, String> {
        self.calls.borrow_mut().push(format!("tag_commit_sha:{tag}"));
        self.tag_commits.get(tag).cloned().ok_or_else(|| format!("tag {tag} does not exist"))
    }

    fn branch_sha(&self, branch: &str) -> Result<String, String> {
        self.calls.borrow_mut().push(format!("branch_sha:{branch}"));
        Ok(self.branch_shas.get(branch).cloned().unwrap_or_else(|| self.head_sha.clone()))
    }
}

pub struct MockHosting {
    pub calls: RefCell<Vec<String>>,
    /// (head, base) -> url of an OPEN PR, for `open_pr_to`.
    pub open_prs_to: HashMap<(String, String), String>,
    /// When set, `open_url` fails with this (the call is still recorded).
    pub open_url_error: Option<String>,
    /// When set, `copy_text` fails with this (the call is still recorded).
    pub copy_text_error: Option<String>,
    pub pr_url: String,
    /// What `merged_pr` reports (defaults to no merged PR).
    pub merged_pr: Option<gflow::hosting::MergedPr>,
    /// What `merged_pr_to` reports, keyed by (head, base) (defaults to none landed).
    pub merged_prs_to: HashMap<(String, String), gflow::hosting::LandedPr>,
}

impl MockHosting {
    pub fn new() -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            open_prs_to: HashMap::new(),
            open_url_error: None,
            copy_text_error: None,
            pr_url: "https://github.com/org/repo/pull/1".to_string(),
            merged_pr: None,
            merged_prs_to: HashMap::new(),
        }
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }
}

impl HostingPlatform for MockHosting {
    fn create_or_get_pr(&self, head: &str, base: &str, title: &str, body: PrBody<'_>) -> Result<String, String> {
        let suffix = match body {
            PrBody::File(t) => format!(":template={t}"),
            PrBody::NativeDefault => String::new(),
            PrBody::Empty => ":empty-body".to_string(),
        };
        self.calls.borrow_mut().push(format!("create_or_get_pr:{head}:{base}:{title}{suffix}"));
        Ok(self.pr_url.clone())
    }

    fn merged_pr(&self, head: &str) -> Result<Option<gflow::hosting::MergedPr>, String> {
        self.calls.borrow_mut().push(format!("merged_pr:{head}"));
        Ok(self.merged_pr.clone())
    }

    fn open_pr_to(&self, head: &str, base: &str) -> Result<Option<String>, String> {
        self.calls.borrow_mut().push(format!("open_pr_to:{head}:{base}"));
        Ok(self.open_prs_to.get(&(head.to_string(), base.to_string())).cloned())
    }

    fn merged_pr_to(&self, head: &str, base: &str) -> Result<Option<gflow::hosting::LandedPr>, String> {
        self.calls.borrow_mut().push(format!("merged_pr_to:{head}:{base}"));
        Ok(self.merged_prs_to.get(&(head.to_string(), base.to_string())).cloned())
    }

    fn open_url(&self, url: &str) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("open_url:{url}"));
        match &self.open_url_error {
            Some(e) => Err(e.clone()),
            None => Ok(()),
        }
    }

    fn copy_text(&self, text: &str) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("copy_text:{text}"));
        match &self.copy_text_error {
            Some(e) => Err(e.clone()),
            None => Ok(()),
        }
    }

    fn check_auth(&self) -> Result<(), String> {
        self.calls.borrow_mut().push("check_auth".to_string());
        Ok(())
    }
}

pub struct MockWorktreeSetup {
    pub calls: RefCell<Vec<String>>,
    /// Commands that fail with `Err("boom")`.
    pub fail: HashSet<String>,
}

impl MockWorktreeSetup {
    pub fn new() -> Self {
        Self { calls: RefCell::new(Vec::new()), fail: HashSet::new() }
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }
}

impl gflow::worktree_setup::WorktreeSetup for MockWorktreeSetup {
    fn run_command(&self, worktree: &Path, main_root: &Path, command: &str) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("run:{}:{}:{command}", worktree.display(), main_root.display()));
        if self.fail.contains(command) { Err("boom".to_string()) } else { Ok(()) }
    }
}

pub struct MockEditor {
    pub calls: RefCell<Vec<String>>,
    /// When true, `open` returns an error (simulates editor not on PATH).
    pub fail: bool,
}

impl MockEditor {
    pub fn new() -> Self {
        Self { calls: RefCell::new(Vec::new()), fail: false }
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }
}

impl Editor for MockEditor {
    fn open(&self, path: &Path) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("open:{}", path.display()));
        if self.fail {
            Err("editor failed".to_string())
        } else {
            Ok(())
        }
    }
}

pub struct MockVersionScript {
    pub calls: RefCell<Vec<String>>,
    /// When set, `run` returns this error instead of succeeding.
    pub fail: Option<String>,
    /// When set, only the Nth `run` call (1-indexed) fails; others succeed.
    /// Lets a test exercise a script that succeeds on an earlier call in the
    /// same flow (e.g. M1) and fails on a later one (e.g. M2) — `fail` alone
    /// cannot express that, since it applies to every call uniformly.
    pub fail_nth_run: Option<u32>,
    run_call_count: RefCell<u32>,
}

impl MockVersionScript {
    pub fn new() -> Self {
        Self { calls: RefCell::new(Vec::new()), fail: None, fail_nth_run: None, run_call_count: RefCell::new(0) }
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }
}

impl VersionScript for MockVersionScript {
    fn run(&self, version: &str) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("run:{version}"));
        let mut count = self.run_call_count.borrow_mut();
        *count += 1;
        if Some(*count) == self.fail_nth_run {
            return Err(format!("version script failed on run #{count}"));
        }
        match &self.fail {
            Some(e) => Err(e.clone()),
            None => Ok(()),
        }
    }

    fn display_name(&self) -> String {
        "set-version.sh".to_string()
    }
}

/// Scripted `Prompter`: records every prompt and answers from a queue. An
/// unscripted prompt is an error, so a test proves a flow never asked simply by
/// not scripting anything.
pub struct MockPrompter {
    pub calls: RefCell<Vec<String>>,
    pub selections: RefCell<VecDeque<usize>>,
    pub lines: RefCell<VecDeque<String>>,
    /// Every prompt returns `Err("Aborted")`, as Ctrl-C/Esc do.
    pub abort: bool,
}

impl MockPrompter {
    pub fn new() -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            selections: RefCell::new(VecDeque::new()),
            lines: RefCell::new(VecDeque::new()),
            abort: false,
        }
    }

    pub fn scripted(selections: &[usize]) -> Self {
        let p = Self::new();
        p.selections.borrow_mut().extend(selections.iter().copied());
        p
    }

    /// Queue the answers to `prompt_name` / `prompt_line`, in order.
    pub fn with_lines(self, lines: &[&str]) -> Self {
        self.lines.borrow_mut().extend(lines.iter().map(|s| s.to_string()));
        self
    }

    pub fn aborting() -> Self {
        Self { abort: true, ..Self::new() }
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }

    fn next_line(&self, kind: &str, prompt: &str) -> Result<String, String> {
        self.calls.borrow_mut().push(format!("{kind}:{prompt}"));
        if self.abort {
            return Err("Aborted".to_string());
        }
        self.lines.borrow_mut().pop_front()
            .ok_or_else(|| format!("MockPrompter: unscripted {kind}('{prompt}')"))
    }
}

impl Prompter for MockPrompter {
    fn select(&self, prompt: &str, items: &[&str]) -> Result<usize, String> {
        self.calls.borrow_mut().push(format!("select:{prompt}:[{}]", items.join(", ")));
        if self.abort {
            return Err("Aborted".to_string());
        }
        self.selections.borrow_mut().pop_front()
            .ok_or_else(|| format!("MockPrompter: unscripted select('{prompt}')"))
    }

    fn prompt_name(&self, prompt: &str) -> Result<String, String> {
        let name = self.next_line("prompt_name", prompt)?;
        // The trait promises a name that passes validate_branch_name; the real
        // prompter re-prompts until it does, so a flow can never see an invalid one.
        assert!(
            validate_branch_name(&name).is_ok(),
            "MockPrompter scripted '{name}' for prompt_name, which the real prompter \
             would have rejected and re-prompted for: {:?}",
            validate_branch_name(&name),
        );
        Ok(name)
    }

    fn prompt_line(&self, prompt: &str) -> Result<String, String> {
        self.next_line("prompt_line", prompt)
    }
}

/// Scripted hosting CLI: records each invocation as `"<program> <args…>"` and
/// answers from a queue of results, so provider policy (reuse vs. create, which
/// failures are fatal) is testable without `gh` or `az` installed.
pub struct MockCliRunner {
    pub calls: RefCell<Vec<String>>,
    responses: RefCell<VecDeque<Result<String, String>>>,
}

impl MockCliRunner {
    pub fn scripted(responses: &[Result<&str, &str>]) -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            responses: RefCell::new(
                responses.iter()
                    .map(|r| r.map(str::to_string).map_err(str::to_string))
                    .collect(),
            ),
        }
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }
}

impl CliRunner for MockCliRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<String, String> {
        self.calls.borrow_mut().push(format!("{program} {}", args.join(" ")));
        self.responses.borrow_mut().pop_front()
            .unwrap_or_else(|| Err(format!("MockCliRunner: unscripted call to {program}")))
    }
}

/// Scripted `git` process: records each invocation as `"git <args…>"` and
/// answers from a queue of (exit code, stdout, stderr). Exit codes are explicit
/// because `GitCli` reads them directly — 1 means "not set" for `config --get`,
/// 5 means "already unset" for `config --unset`.
pub struct MockCommandRunner {
    pub calls: RefCell<Vec<String>>,
    responses: RefCell<VecDeque<CliOutput>>,
}

impl MockCommandRunner {
    /// One successful invocation returning `stdout`.
    pub fn ok(stdout: &str) -> Self {
        Self::scripted(&[(0, stdout, "")])
    }

    pub fn scripted(responses: &[(i32, &str, &str)]) -> Self {
        Self::from_outputs(responses.iter().map(|(code, out, err)| CliOutput {
            code: Some(*code),
            stdout: out.to_string(),
            stderr: err.to_string(),
        }))
    }

    /// A process killed by a signal — no exit code at all.
    pub fn terminated_by_signal() -> Self {
        Self::from_outputs(std::iter::once(CliOutput {
            code: None,
            stdout: String::new(),
            stderr: String::new(),
        }))
    }

    fn from_outputs(outputs: impl Iterator<Item = CliOutput>) -> Self {
        Self { calls: RefCell::new(Vec::new()), responses: RefCell::new(outputs.collect()) }
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }
}

impl CommandRunner for MockCommandRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<CliOutput, String> {
        self.calls.borrow_mut().push(format!("{program} {}", args.join(" ")));
        self.responses.borrow_mut().pop_front()
            .ok_or_else(|| format!("MockCommandRunner: unscripted call to {program} {}", args.join(" ")))
    }
}

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Deletes itself on drop, so cleanup also runs when a test panics.
pub struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

impl std::ops::Deref for TempDir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

/// Unique temp directory for integration tests that need a fake `.git` dir
/// (state files). Mirrors the lib's #[cfg(test)] helper, which integration
/// tests cannot link.
pub fn tmp_dir(prefix: &str) -> TempDir {
    let n = TMP_COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("{prefix}-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    TempDir(dir)
}

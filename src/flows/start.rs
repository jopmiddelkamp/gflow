use crate::flows::{announce_version_pr, open_versioned_branches, require_clean_tree, run_version_script};
use crate::git::Git;
use crate::hosting::{HostingPlatform, PrBody};
use crate::prompt::Prompter;
use crate::repo_config::{BumpStrategy, Mode, RepoConfig};
use crate::version::SemVer;
use crate::version_script::VersionScript;
use crate::worktree::{open_worktree, WorktreeContext};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ReleaseType {
    Major,
    Minor,
}

/// An active worktree context leaves the current checkout untouched, exactly
/// like `--no-checkout`. This is the flows' single derivation of that rule
/// (pinned by the worktree tests: passing no_checkout=false with a worktree
/// context must still take the no-checkout path).
fn effective_no_checkout(no_checkout: bool, worktree: &Option<WorktreeContext<'_>>) -> bool {
    no_checkout || worktree.is_some()
}

fn materialize_branch(
    git: &dyn Git,
    branch: &str,
    base: &str,
    no_checkout: bool,
    worktree: Option<WorktreeContext<'_>>,
) -> Result<(), String> {
    println!("Creating branch: {branch}");
    if no_checkout {
        git.create_branch_no_checkout(branch, base)?;
    } else {
        git.create_branch(branch, base)?;
    }
    git.push(branch)?;
    println!("✔ Branch '{branch}' created and pushed.");
    if let Some(ctx) = worktree {
        open_worktree(git, &ctx, branch)?;
    }
    Ok(())
}

/// `list_branches_matching` strips the `origin/` prefix, so a discovered branch
/// that exists only on the remote is not an object `git branch <new> <base>`
/// can resolve. Local wins when both exist: it is what the user would have
/// branched from by hand.
///
/// DRY: `open_versioned_branches` resolves the same short name origin-first,
/// because "has it shipped" is a question only the remote can answer.
fn branchable_ref(git: &dyn Git, branch: &str) -> Result<String, String> {
    if git.local_branch_exists(branch)? {
        Ok(branch.to_string())
    } else {
        Ok(format!("origin/{branch}"))
    }
}

/// The container branch HEAD is on, when it carries `prefix`. Standing on a
/// release or hotfix is an explicit choice, so it beats discovery — which is
/// only ever a fallback for running the command from somewhere else.
fn standing_on(git: &dyn Git, prefix: &str) -> Result<Option<String>, String> {
    let current = git.current_branch()?;
    Ok(current.starts_with(prefix).then_some(current))
}

/// A parent with no parseable version can only yield a fix branch that
/// `BranchType::parse` reads back as `Other` — creatable, never finishable.
fn version_of(branch: &str, prefix: &str) -> Result<SemVer, String> {
    branch.strip_prefix(prefix)
        .and_then(SemVer::parse)
        .ok_or_else(|| format!("Branch '{branch}' does not carry a version; cannot derive a fix branch from it."))
}

pub fn start_work_branch(git: &dyn Git, prefix: &str, name: &str, from: &str, no_checkout: bool, worktree: Option<WorktreeContext<'_>>) -> Result<(), String> {
    let branch = format!("{prefix}/{name}");
    let effective_no_checkout = effective_no_checkout(no_checkout, &worktree);
    // `from` is user-supplied, so git's "not a commit" becomes guidance naming --base.
    materialize_branch(git, &branch, from, effective_no_checkout, worktree).map_err(|e| {
        if e.contains("not a commit") {
            format!("Branch '{from}' does not exist. Use --base to specify a different base branch.")
        } else {
            e
        }
    })
}

/// With a worktree context the release is still created in the current tree
/// (the version script needs the branch checked out), then the tree returns to
/// develop and the release opens in its own worktree — or, when a worktree
/// already holds it, that one is announced.
pub fn start_release(git: &dyn Git, prompter: &dyn Prompter, hosting: &dyn HostingPlatform, script: Option<&dyn VersionScript>, cfg: &RepoConfig, release_type: Option<ReleaseType>, main_branch: &str, worktree: Option<WorktreeContext<'_>>) -> Result<(), String> {
    let branch = resolve_or_create_release(git, prompter, hosting, script, cfg, release_type, main_branch, worktree.is_some())?;
    if let Some(ctx) = worktree {
        open_container_worktree(git, &ctx, "Release", &branch)?;
    }
    Ok(())
}

pub fn start_release_fix(git: &dyn Git, hosting: &dyn HostingPlatform, cfg: &RepoConfig, main_branch: &str, name: &str, no_checkout: bool, worktree: Option<WorktreeContext<'_>>) -> Result<(), String> {
    let effective_no_checkout = effective_no_checkout(no_checkout, &worktree);
    let (release_branch, base) = match standing_on(git, "release/")? {
        Some(current) => (current.clone(), current),
        None if effective_no_checkout => {
            let discovered = open_versioned_branches(git, hosting, cfg, main_branch, "release")?
                .first()
                .ok_or("No release branch found. Create one with 'gflow start release' first.")?
                .clone();
            let base = branchable_ref(git, &discovered)?;
            (discovered, base)
        }
        None => return Err("Not on a release branch".to_string()),
    };

    let branch = version_of(&release_branch, "release/")?.release_fix_branch(name);
    materialize_branch(git, &branch, &base, effective_no_checkout, worktree)
}

/// In worktree mode the hotfix container gets its own worktree too (opened
/// before the fix branch's, which then keeps editor focus): `start hotfix-fix`
/// is the only command that creates a hotfix container, so no later command
/// would hand it one — and `gflow finish` needs it checked out somewhere.
pub fn start_hotfix_fix(git: &dyn Git, hosting: &dyn HostingPlatform, cfg: &RepoConfig, name: &str, no_checkout: bool, worktree: Option<WorktreeContext<'_>>, main_branch: &str, script: Option<&dyn VersionScript>) -> Result<(), String> {
    let effective_no_checkout = effective_no_checkout(no_checkout, &worktree);
    let (hotfix_branch, base) = match standing_on(git, "hotfix/")? {
        Some(current) => (current.clone(), current),
        None => resolve_or_create_hotfix(git, hosting, cfg, effective_no_checkout, main_branch, script)?,
    };
    let branch = version_of(&hotfix_branch, "hotfix/")?.hotfix_fix_branch(name);
    if let Some(ctx) = &worktree {
        open_container_worktree(git, ctx, "Hotfix", &hotfix_branch)?;
    }
    materialize_branch(git, &branch, &base, effective_no_checkout, worktree)
}

/// A release/hotfix container branch handed to worktree mode: open it in its
/// own worktree, or announce the worktree that already holds it.
fn open_container_worktree(git: &dyn Git, ctx: &WorktreeContext<'_>, label: &str, branch: &str) -> Result<(), String> {
    match git.worktree_of(branch)? {
        Some(path) => {
            println!("{label} branch {branch} is already open at {}", path.display());
            Ok(())
        }
        None => open_worktree(git, ctx, branch),
    }
}

/// `hand_off`: the caller will move the release into a worktree, so the current
/// tree must not be left on it — no checkout of an existing release, and a
/// return to develop after creating a new one.
fn resolve_or_create_release(git: &dyn Git, prompter: &dyn Prompter, hosting: &dyn HostingPlatform, script: Option<&dyn VersionScript>, cfg: &RepoConfig, release_type: Option<ReleaseType>, main_branch: &str, hand_off: bool) -> Result<String, String> {
    let release_branches = open_versioned_branches(git, hosting, cfg, main_branch, "release")?;

    if let Some(branch) = release_branches.first() {
        println!("Using existing release branch: {branch}");
        if !hand_off {
            git.checkout(branch)?;
        }
        return Ok(branch.to_string());
    }

    let latest = find_latest_tag(git)?;
    let next = match release_type {
        Some(ReleaseType::Major) => latest.bump_major(),
        Some(ReleaseType::Minor) => latest.bump_minor(),
        None => {
            let has_breaking = detect_breaking_changes(git, &latest);
            prompt_release_type(prompter, &latest, has_breaking)?
        }
    };

    let branch = next.release_branch();
    let first_tag = match cfg.bump_strategy {
        BumpStrategy::Rc => next.with_rc(1),
        BumpStrategy::Patch => next.clone(),
    };
    let tag = first_tag.tag_name();

    if script.is_some() {
        require_clean_tree(git)?;
    }
    println!("Creating release branch: {branch}");
    git.checkout("develop")?;
    git.create_branch(&branch, "develop")?;
    if let Some(script) = script {
        run_version_script(git, script, &next)?;
    }
    git.push(&branch)?;

    println!("Tagging: {tag}");
    git.create_tag(&tag, &format!("chore: create release branch {next}"))?;
    git.push_tag(&tag)?;

    if let Some(script) = script {
        let dev = next.bump_minor();
        if let Err(e) = bump_develop(git, hosting, script, cfg, &dev, &branch) {
            eprintln!("Warning: develop version bump failed: {e}");
            eprintln!("{}", m2_failure_advice(cfg.mode, &script.display_name(), &dev));
            let _ = git.checkout(&branch);
        }
    }
    if hand_off {
        git.checkout("develop")?;
    }

    Ok(branch)
}

/// M2 warn-and-continue advice: how to finish the develop version bump by
/// hand after a failure. Free mode can commit and push develop directly;
/// protected mode never pushes develop (gflow SKILL.md, "Landing modes"), so a
/// direct push there would just be rejected — the fix must go out as its own PR.
fn m2_failure_advice(mode: Mode, script_name: &str, version: &SemVer) -> String {
    match mode {
        Mode::Free => format!(
            "The release was created successfully. Update develop's version manually: run {script_name} {version} on develop and commit."
        ),
        Mode::Protected => format!(
            "The release was created successfully. Update develop's version manually: branch from develop, run {script_name} {version}, commit, and open a PR to develop."
        ),
    }
}

/// Moment 2: after the release is cut (and its rc.1 tag pushed), bump develop
/// to the next dev version so it never regresses behind the release branch.
/// Always ends back on `release_branch` on its own success paths; a failure
/// here is caught by the caller, which restores the checkout itself.
fn bump_develop(git: &dyn Git, hosting: &dyn HostingPlatform, script: &dyn VersionScript, cfg: &RepoConfig, dev: &SemVer, release_branch: &str) -> Result<(), String> {
    git.checkout("develop")?;
    git.ff_merge("origin/develop")?;

    match cfg.mode {
        Mode::Free => {
            require_clean_tree(git)?;
            if run_version_script(git, script, dev)? {
                git.push("develop")?;
            } else {
                println!("↷ skipped: develop version bump (no changes)");
            }
        }
        Mode::Protected => bump_develop_protected(git, hosting, script, dev)?,
    }

    git.checkout(release_branch)
}

/// Protected-mode M2: never push develop directly. Reuses a leftover
/// `chore/set-version-*` branch instead of recreating it — the resume case
/// after a prior run's crash between branch creation and the version PR.
fn bump_develop_protected(git: &dyn Git, hosting: &dyn HostingPlatform, script: &dyn VersionScript, dev: &SemVer) -> Result<(), String> {
    let chore_branch = format!("chore/set-version-{dev}");
    let title = format!("chore: set version {dev}");

    if git.remote_branch_exists(&chore_branch)? {
        let url = hosting.create_or_get_pr(&chore_branch, "develop", &title, PrBody::NativeDefault)?;
        announce_version_pr(hosting, &title, &url);
        return Ok(());
    }

    // A prior run can leave this branch behind locally (created, then
    // interrupted before the script committed or pushed) — machine-owned, so
    // gflow clears it itself rather than dying on git's raw "branch already
    // exists" (mirrors bump_protected in finish_release.rs).
    if git.local_branch_exists(&chore_branch)? {
        git.delete_branch_local(&chore_branch)?;
    }
    git.create_branch(&chore_branch, "develop")?;
    require_clean_tree(git)?;
    match run_version_script(git, script, dev) {
        Ok(true) => {
            git.push(&chore_branch)?;
            let url = hosting.create_or_get_pr(&chore_branch, "develop", &title, PrBody::NativeDefault)?;
            announce_version_pr(hosting, &title, &url);
            Ok(())
        }
        Ok(false) => {
            git.checkout("develop")?;
            git.delete_branch_local(&chore_branch)?;
            println!("↷ skipped: develop version bump (no changes)");
            Ok(())
        }
        Err(e) => {
            // Best-effort: restore develop so the caller's own final checkout
            // (back to the release branch) still runs from a sane place.
            let _ = git.checkout("develop");
            Err(e)
        }
    }
}

pub fn detect_breaking_changes(git: &dyn Git, latest: &SemVer) -> bool {
    let tag = latest.tag_name();
    // Scan develop explicitly — start release may run from any branch
    let messages = match git.commit_messages(&tag, "develop") {
        Ok(msgs) => msgs,
        Err(_) => match git.commit_messages(&tag, "origin/develop") {
            Ok(msgs) => msgs,
            Err(_) => return false,
        },
    };

    for msg in &messages {
        if message_is_breaking(msg) {
            return true;
        }
    }

    false
}

pub(crate) fn message_is_breaking(msg: &str) -> bool {
    let mut lines = msg.lines();
    let first_line = lines.next().unwrap_or("");

    // Conventional commits: type or type(scope) followed by "!:" e.g. "feat!:", "refactor(auth)!:"
    if let Some(colon_pos) = first_line.find(':') {
        let before_colon = &first_line[..colon_pos];
        if before_colon.ends_with('!') {
            return true;
        }
    }

    // Conventional commits footer: a line starting with "BREAKING CHANGE:" or "BREAKING-CHANGE:"
    // (case-insensitive, followed by a colon and space per the spec)
    for line in lines {
        let trimmed = line.trim_start();
        let upper = trimmed.to_uppercase();
        if upper.starts_with("BREAKING CHANGE:") || upper.starts_with("BREAKING-CHANGE:") {
            return true;
        }
    }

    false
}

fn prompt_release_type(prompter: &dyn Prompter, latest: &SemVer, has_breaking: bool) -> Result<SemVer, String> {
    let major_label = format!("major (v{} → v{})", latest, latest.bump_major());
    let minor_label = format!("minor (v{} → v{})", latest, latest.bump_minor());

    let items: Vec<&str> = if has_breaking {
        println!("Breaking changes detected since last release.");
        vec![&major_label, &minor_label]
    } else {
        vec![&minor_label, &major_label]
    };

    let idx = prompter.select("Release type", &items)?;
    let selected = items[idx];

    if selected.starts_with("major") {
        Ok(latest.bump_major())
    } else {
        Ok(latest.bump_minor())
    }
}

/// Returns the hotfix branch and the ref to cut the fix from. They differ only
/// for a reused branch that was never checked out here: a checkout creates the
/// local branch from origin on its own, `git branch` does not.
fn resolve_or_create_hotfix(git: &dyn Git, hosting: &dyn HostingPlatform, cfg: &RepoConfig, no_checkout: bool, main_branch: &str, script: Option<&dyn VersionScript>) -> Result<(String, String), String> {
    let hotfix_branches = open_versioned_branches(git, hosting, cfg, main_branch, "hotfix")?;

    if let Some(branch) = hotfix_branches.first() {
        println!("Using existing hotfix branch: {branch}");
        let base = if no_checkout {
            branchable_ref(git, branch)?
        } else {
            git.checkout(branch)?;
            branch.clone()
        };
        return Ok((branch.clone(), base));
    }

    let latest = match cfg.bump_strategy {
        BumpStrategy::Rc => find_latest_tag(git)?,
        BumpStrategy::Patch => find_latest_shipped_tag(git, hosting, cfg, main_branch)?,
    };
    let next = latest.bump_patch();
    let branch = next.hotfix_branch();

    if !no_checkout && script.is_some() {
        require_clean_tree(git)?;
    }
    println!("Creating hotfix branch: {branch}");
    if no_checkout {
        git.create_branch_no_checkout(&branch, main_branch)?;
        if let Some(script) = script {
            eprintln!(
                "⚠ Version script not run: {branch} was created without checkout, so gflow cannot commit version files there."
            );
            eprintln!(
                "  Recover manually: git switch {branch}, run {} {next}, commit, and push.",
                script.display_name(),
            );
        }
    } else {
        git.checkout(main_branch)?;
        git.create_branch(&branch, main_branch)?;
        if let Some(script) = script {
            run_version_script(git, script, &next)?;
        }
    }
    git.push(&branch)?;

    Ok((branch.clone(), branch))
}

/// Patch-strategy sibling of `find_latest_tag` for hotfix versioning: every
/// tag is clean under patch, so an open release branch's staging tags
/// (`v2.6.0`, `v2.6.1`, …) would win the global max while production still
/// runs `v2.5.3` — the hotfix would misversion itself and steal the number the
/// release's next bump computes. Tags whose `major.minor` matches an open
/// (unshipped) release branch are that release's staging history, not
/// production's, and are excluded. Under rc this filter is provably empty (an
/// open release carries only `-rc.N` tags), which is why rc keeps the plain
/// global scan.
fn find_latest_shipped_tag(git: &dyn Git, hosting: &dyn HostingPlatform, cfg: &RepoConfig, main_branch: &str) -> Result<SemVer, String> {
    let in_flight: Vec<SemVer> = crate::flows::open_versioned_branches(git, hosting, cfg, main_branch, "release")?
        .iter()
        .filter_map(|b| b.strip_prefix("release/").and_then(SemVer::parse))
        .collect();
    let tags = git.list_tags()?;
    Ok(tags
        .iter()
        .filter_map(|t| SemVer::parse(t))
        .filter(|v| !v.is_pre_release())
        .filter(|v| !in_flight.iter().any(|r| r.major == v.major && r.minor == v.minor))
        .max()
        .unwrap_or_else(|| SemVer::new(0, 0, 0)))
}

fn find_latest_tag(git: &dyn Git) -> Result<SemVer, String> {
    let tags = git.list_tags()?;
    let all: Vec<SemVer> = tags.iter().filter_map(|t| SemVer::parse(t)).collect();

    // Prefer clean release tags
    if let Some(v) = all.iter().filter(|v| !v.is_pre_release()).max() {
        return Ok(v.clone());
    }

    // Fall back to highest RC tag (stripped to release) if no clean tags exist
    if let Some(v) = all.iter().filter(|v| v.is_rc()).max() {
        return Ok(v.to_release());
    }

    Ok(SemVer::new(0, 0, 0))
}

#[cfg(test)]
mod tests {
    use super::{message_is_breaking, m2_failure_advice};
    use crate::repo_config::Mode;
    use crate::version::SemVer;

    #[test]
    fn m2_failure_advice_free_mode_names_the_direct_commit() {
        let msg = m2_failure_advice(Mode::Free, "set-version.sh", &SemVer::new(1, 2, 0));
        assert_eq!(msg, "The release was created successfully. Update develop's version manually: run set-version.sh 1.2.0 on develop and commit.");
    }

    #[test]
    fn m2_failure_advice_protected_mode_names_a_pr() {
        let msg = m2_failure_advice(Mode::Protected, "set-version.sh", &SemVer::new(1, 2, 0));
        assert_eq!(msg, "The release was created successfully. Update develop's version manually: branch from develop, run set-version.sh 1.2.0, commit, and open a PR to develop.");
    }

    #[test]
    fn bang_in_title() {
        assert!(message_is_breaking("feat!: remove legacy API"));
    }

    #[test]
    fn bang_with_scope() {
        assert!(message_is_breaking("refactor(auth)!: rewrite token handling"));
    }

    #[test]
    fn breaking_change_footer() {
        let msg = "feat: new auth flow\n\nBREAKING CHANGE: old tokens are invalidated";
        assert!(message_is_breaking(msg));
    }

    #[test]
    fn breaking_change_footer_hyphenated() {
        let msg = "feat: new auth\n\nBREAKING-CHANGE: old tokens invalidated";
        assert!(message_is_breaking(msg));
    }

    #[test]
    fn breaking_change_footer_case_insensitive() {
        let msg = "feat: new auth\n\nbreaking change: old tokens invalidated";
        assert!(message_is_breaking(msg));
    }

    #[test]
    fn non_breaking_change_in_body_is_not_flagged() {
        // This is the bug the reviewer caught — "non-breaking change" should NOT match
        let msg = "feat: new feature\n\nThis is a non-breaking change to the API.";
        assert!(!message_is_breaking(msg));
    }

    #[test]
    fn breaking_change_mention_without_colon_is_not_flagged() {
        // Only the footer format (with colon) should count
        let msg = "feat: new feature\n\nWe discussed breaking change options earlier.";
        assert!(!message_is_breaking(msg));
    }

    #[test]
    fn plain_conventional_commit_is_not_breaking() {
        assert!(!message_is_breaking("feat: add login page"));
        assert!(!message_is_breaking("fix: correct typo"));
        assert!(!message_is_breaking("chore: update deps"));
    }

    #[test]
    fn bang_in_body_does_not_count() {
        // The ! must be in the title before the colon, not in the body
        let msg = "feat: add feature\n\nThis is great!\nReally awesome.";
        assert!(!message_is_breaking(msg));
    }
}

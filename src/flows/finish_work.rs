use std::path::Path;

use crate::flows::{completion_instruction, copy_pr_to_clipboard, enforce_completion_type, open_pr_in_browser, pr_block, CompletionType, WORK_PR_UNDO};
use crate::git::Git;
use crate::hosting::PrBody;
use crate::git::branch::BranchType;
use crate::hosting::HostingPlatform;
use crate::prompt::Prompter;
use crate::version::SemVer;

/// If the branch's PR is already merged, the work is done — finish by cleaning
/// up instead of opening a new PR. Completion is derived from the hosting
/// platform's PR state (no state file), guarded by a head-SHA match: only the
/// exact commit that was merged is safe to delete; new commits since the merge
/// mean new work, which continues into a fresh PR.
///
/// Returns `true` when cleanup ran and the finish is complete.
fn try_cleanup_merged(git: &dyn Git, hosting: &dyn HostingPlatform, current: &str, accept_merge_type: bool) -> Result<bool, String> {
    let Some(pr) = hosting.merged_pr(current)? else {
        return Ok(false);
    };
    if git.head_sha()? != pr.head_sha {
        println!("PR {} was merged, but this branch has new commits since — creating a new PR.", pr.url);
        return Ok(false);
    }
    println!("PR already merged: {}", pr.url);
    // Wrong completion type stops the finish BEFORE cleanup: the branch is
    // still needed to redo the PR, so nothing may be deleted yet.
    enforce_completion_type(git, &pr.url, &pr.merge_commit_sha, CompletionType::Squash, accept_merge_type, WORK_PR_UNDO)?;

    // Remote deletion first: after the worktree is removed the process working
    // directory is gone, so every other git call must happen before it.
    if git.remote_branch_exists(current)? {
        println!("Deleting remote branch: {current}");
        git.delete_branch_remote(current)?;
    } else {
        println!("↷ skipped: remote branch deletion (already gone)");
    }

    if git.is_linked_worktree()? {
        // Free the branch from this worktree, drop it, then drop the worktree.
        git.detach_head()?;
        println!("Deleting local branch: {current}");
        git.delete_branch_local(current)?;
        let path = git.remove_current_worktree()?;
        println!("Removed worktree: {}", path.display());
        println!("You can close this editor window now.");
    } else {
        println!("Switching to {}", pr.base);
        git.checkout(&pr.base)?;
        // The merge already happened on the remote; a stale local base is a
        // warning, not a failed finish.
        if let Err(e) = git.ff_merge(&format!("origin/{}", pr.base)) {
            eprintln!("Warning: could not fast-forward {}: {e}", pr.base);
        }
        println!("Deleting local branch: {current}");
        git.delete_branch_local(current)?;
    }
    println!("✔ {current} is finished.");
    Ok(true)
}

/// `template` is the pre-resolved PR template path (resolved at the composition
/// root, anchored to the repo root) — flows never probe the filesystem.
fn push_and_create_pr(git: &dyn Git, hosting: &dyn HostingPlatform, current: &str, base: &str, title: &str, template: Option<&Path>) -> Result<(), String> {
    println!("Pushing branch: {current}");
    git.push(current)?;

    if let Some(path) = template {
        println!("Using PR template: {}", path.display());
    }

    println!("Creating PR against {base}...");
    let url = hosting.create_or_get_pr(current, base, title, template.and_then(|p| p.to_str()).map_or(PrBody::NativeDefault, PrBody::File))?;
    copy_pr_to_clipboard(hosting, title, &url);
    println!("\n{}\n", pr_block(title, &url, crate::style::styled()));
    println!("{}", completion_instruction(CompletionType::Squash));
    open_pr_in_browser(hosting, &url);

    Ok(())
}

/// Branch names are hyphenated slugs; PR titles read as prose, so the hyphens
/// become spaces (`feat/foo-bar` → `feat: foo bar`).
fn pr_title(commit_type: &str, breaking: bool, name: &str) -> String {
    let bang = if breaking { "!" } else { "" };
    format!("{commit_type}{bang}: {}", name.replace('-', " "))
}

fn detect_parent_branch(git: &dyn Git, prompter: &dyn Prompter, current: &str) -> Result<String, String> {
    let remote_branches = git.list_remote_branches()?;
    let mut candidates: Vec<(String, u32)> = Vec::new();

    for branch in &remote_branches {
        if branch == current {
            continue;
        }
        // Candidate parents are develop and work branches — decided by the
        // taxonomy in BranchType, not a local prefix list.
        let parsed = BranchType::parse(branch);
        if parsed != BranchType::Develop && !parsed.is_work_branch() {
            continue;
        }
        // chore/set-version-* is gflow-created and machine-owned: a protected
        // version bump merges and deletes it, which would strand a PR that
        // targeted it as a base.
        if branch.starts_with("chore/set-version-") {
            continue;
        }
        let base = match git.merge_base(current, branch) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let current_count = match git.rev_list_count(&base, current) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let candidate_count = match git.rev_list_count(&base, branch) {
            Ok(c) => c,
            Err(_) => continue,
        };
        // Skip child work branches: a candidate that already contains our whole
        // history (nothing of ours is missing from it) while carrying commits
        // of its own branched *from* us. Comparing the two counts instead
        // would drop a busy `develop` — every teammate's merge inflates its
        // count past ours.
        //
        // `develop` is never a child: in this branch model every work branch
        // targets it. Exempting it keeps a develop that already contains our
        // tip (merged outside a PR the host reports) in the menu, matching the
        // no-candidates fallback below, which targets develop too.
        if parsed != BranchType::Develop && current_count == 0 && candidate_count > 0 {
            continue;
        }
        candidates.push((branch.clone(), current_count));
    }

    if candidates.is_empty() {
        return Ok("develop".to_string());
    }

    if candidates.len() == 1 {
        let (branch, _) = candidates.pop().expect("len checked above");
        println!("PR target branch: {branch} (auto-detected)");
        return Ok(branch);
    }

    // Sort by distance ascending; on ties prefer develop, then alphabetical
    candidates.sort_by(|a, b| {
        a.1.cmp(&b.1)
            .then_with(|| {
                let a_is_develop = a.0 == "develop";
                let b_is_develop = b.0 == "develop";
                b_is_develop.cmp(&a_is_develop)
            })
            .then_with(|| a.0.cmp(&b.0))
    });

    let labels: Vec<&str> = candidates.iter().map(|(name, _)| name.as_str()).collect();
    let idx = prompter.select("PR target branch", &labels)?;
    Ok(candidates[idx].0.clone())
}

#[allow(clippy::too_many_arguments)]
pub fn finish_work_branch(git: &dyn Git, hosting: &dyn HostingPlatform, prompter: &dyn Prompter, branch_type: &BranchType, breaking: Option<bool>, base: Option<String>, template: Option<&Path>, accept_merge_type: bool) -> Result<(), String> {
    let commit_type = branch_type.commit_type().ok_or("Cannot finish: not on a work branch")?;
    let name = branch_type.name().ok_or("Cannot finish: branch has no name")?;
    let current = git.current_branch()?;
    // Validate an explicit --base before anything else (cheap, local), but check
    // for an already-merged PR before parent detection and the breaking prompt —
    // a completed finish must not re-ask questions that no longer matter.
    if let Some(base) = &base {
        if *base == current {
            return Err(format!("Base branch '{base}' is the branch being finished; a PR cannot target its own branch."));
        }
        // PRs are created via the hosting platform, so the base must exist on
        // the remote — a local-only branch would fail later at PR creation.
        if !git.remote_branch_exists(base)? {
            return Err(format!("Base branch '{base}' not found on origin. Push it first (or fetch if it exists remotely)."));
        }
    }
    if try_cleanup_merged(git, hosting, &current, accept_merge_type)? {
        return Ok(());
    }
    let base = match base {
        Some(base) => base,
        None => detect_parent_branch(git, prompter, &current)?,
    };

    let bang = match breaking {
        // Explicit flag always honored, for any work type
        Some(b) => b,
        // Prompt only for types that are commonly breaking
        None if commonly_breaking(commit_type) => prompt_breaking_change(prompter)?,
        None => false,
    };

    let title = pr_title(commit_type, bang, name);

    push_and_create_pr(git, hosting, &current, &base, &title, template)
}

fn commonly_breaking(commit_type: &str) -> bool {
    matches!(commit_type, "feat" | "fix" | "refactor")
}

fn prompt_breaking_change(prompter: &dyn Prompter) -> Result<bool, String> {
    let idx = prompter.select("Contains breaking changes?", &["no", "yes"])?;
    Ok(idx == 1)
}

fn finish_fix(git: &dyn Git, hosting: &dyn HostingPlatform, commit_type: &str, name: &str, parent: &str, template: Option<&Path>, accept_merge_type: bool) -> Result<(), String> {
    let current = git.current_branch()?;
    if try_cleanup_merged(git, hosting, &current, accept_merge_type)? {
        return Ok(());
    }
    push_and_create_pr(git, hosting, &current, parent, &pr_title(commit_type, false, name), template)
}

pub fn finish_release_fix(git: &dyn Git, hosting: &dyn HostingPlatform, branch_type: &BranchType, template: Option<&Path>, accept_merge_type: bool) -> Result<(), String> {
    let BranchType::ReleaseFix { major, minor, patch, name } = branch_type else {
        return Err("Cannot finish: not on a release-fix branch".to_string());
    };
    finish_fix(git, hosting, "fix", name, &SemVer::new(*major, *minor, *patch).release_branch(), template, accept_merge_type)
}

pub fn finish_hotfix_fix(git: &dyn Git, hosting: &dyn HostingPlatform, branch_type: &BranchType, template: Option<&Path>, accept_merge_type: bool) -> Result<(), String> {
    let BranchType::HotfixFix { major, minor, patch, name } = branch_type else {
        return Err("Cannot finish: not on a hotfix-fix branch".to_string());
    };
    finish_fix(git, hosting, "fix", name, &SemVer::new(*major, *minor, *patch).hotfix_branch(), template, accept_merge_type)
}

pub fn finish_release_chore(git: &dyn Git, hosting: &dyn HostingPlatform, branch_type: &BranchType, template: Option<&Path>, accept_merge_type: bool) -> Result<(), String> {
    let BranchType::ReleaseChore { major, minor, patch, name } = branch_type else {
        return Err("Cannot finish: not on a release-chore branch".to_string());
    };
    finish_fix(git, hosting, "chore", name, &SemVer::new(*major, *minor, *patch).release_branch(), template, accept_merge_type)
}

#[cfg(test)]
mod tests {
    use super::pr_title;

    #[test]
    fn hyphens_become_spaces() {
        assert_eq!(pr_title("feat", false, "foo-bar"), "feat: foo bar");
    }

    #[test]
    fn breaking_adds_bang_before_colon() {
        assert_eq!(pr_title("feat", true, "drop-legacy-api"), "feat!: drop legacy api");
    }

    #[test]
    fn single_word_name_is_unchanged() {
        assert_eq!(pr_title("chore", false, "cleanup"), "chore: cleanup");
    }
}

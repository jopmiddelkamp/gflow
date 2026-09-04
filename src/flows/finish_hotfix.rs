use std::path::Path;

use crate::flows::{
    announce_pending_landing, cleanup_finish_branches, delete_source_branch, ensure_finish_branch,
    finish_conflict_hint, finish_leg_landed, land_leg_strict, landing_pr_body, merge_into, refuse_open_legacy_pr, LegState,
    open_versioned_branches, push_if_needed, push_tag_if_missing, report_commits_past_landing, resume_hint, tag_at_if_missing,
    tag_if_missing, tip_landed_somewhere,
};
use crate::git::Git;
use crate::hosting::{HostingPlatform, LandedPr};
use crate::repo_config::{Mode, RepoConfig};
use crate::version::{finish_branch_name, SemVer};

#[allow(clippy::too_many_arguments)]
pub fn finish_hotfix(
    git: &dyn Git,
    hosting: &dyn HostingPlatform,
    cfg: &RepoConfig,
    major: u32,
    minor: u32,
    patch: u32,
    main_branch: &str,
    template: Option<&Path>,
    accept_merge_type: bool,
) -> Result<(), String> {
    if cfg.mode == Mode::Protected {
        return finish_hotfix_protected(git, hosting, cfg, major, minor, patch, main_branch, template, accept_merge_type);
    }

    let version = SemVer::new(major, minor, patch);
    let hotfix_branch = version.hotfix_branch();
    let tag = version.tag_name();

    println!("Finishing hotfix {hotfix_branch}...");

    merge_into(git, &hotfix_branch, main_branch,
        &format!("chore: merge hotfix {version} into {main_branch}"),
        &resume_hint(&hotfix_branch))?;
    tag_if_missing(git, &tag, &format!("chore: hotfix {version}"))?;
    push_if_needed(git, main_branch)?;
    push_tag_if_missing(git, &tag)?;

    merge_into(git, &hotfix_branch, "develop",
        &format!("chore: merge hotfix {version} into develop"),
        &resume_hint(&hotfix_branch))?;
    push_if_needed(git, "develop")?;

    // `release/*` cannot match `release-fix/*`, so no extra filtering is needed
    // for that; `open_versioned_branches` also drops any release already shipped.
    // Sorted despite the trait contract already promising it: deterministic
    // replay on resume is a crash-safety invariant, not something to delegate.
    let mut release_branches = open_versioned_branches(git, hosting, cfg, main_branch, "release")?;
    release_branches.sort();

    for release in &release_branches {
        merge_into(git, &hotfix_branch, release,
            &format!("chore: merge hotfix {version} into {release}"),
            &format!(
                "Hotfix {version} was merged into {main_branch} and develop, but propagation into {release} failed.\n\
                 {}\n\
                 (After all releases are updated, run 'gflow bump' on each to cut a fresh RC for staging.)",
                resume_hint(&hotfix_branch)
            ))?;
        // The push sits outside the merge guard so merged-but-unpushed crashes
        // still push on resume (see decisions.md).
        push_if_needed(git, release)?;
    }

    finish_hotfix_cleanup(git, cfg, &hotfix_branch, main_branch, &version, &release_branches, true)
}

/// `tip_landed` gates deletion: free mode always passes `true` (its own
/// ancestor-based merge guard already proves the branch merged, so there is
/// no separate "did the tip go anywhere" question to ask).
pub fn finish_hotfix_cleanup(git: &dyn Git, cfg: &RepoConfig, hotfix_branch: &str, main_branch: &str, version: &SemVer, release_branches: &[String], tip_landed: bool) -> Result<(), String> {
    println!("Cleaning up hotfix branch...");
    if !tip_landed {
        eprintln!("⚠ Keeping {hotfix_branch}: its tip is not part of any landed pull request, so deleting it could lose commits.");
        eprintln!("  Review it, then delete it yourself: git push origin --delete {hotfix_branch}");
    } else if cfg.keep_release_branches {
        println!("Keeping {hotfix_branch} (keep-release-branches=true).");
    } else {
        if let Some(path) = delete_source_branch(git, hotfix_branch, main_branch)? {
            println!("Removed worktree: {}", path.display());
            println!("You can close this editor window now.");
        }
    }

    if release_branches.is_empty() {
        println!("✔ Hotfix {version} complete.");
    } else {
        let list = release_branches.join(", ");
        println!("✔ Hotfix {version} propagated to: {main_branch}, develop, {list}");
        println!("Run 'gflow bump' on each release branch to cut a new RC.");
    }
    Ok(())
}

/// Protected mode: hotfixes carry no RC gate (unlike releases, `finish_release.rs`),
/// so a landing step opens straight into the same sequential PR-per-run shape —
/// main, then develop, then each open release branch in sorted order, one PR per
/// run, stopping at the first still-pending target. The tag is checked for
/// mainline containment *before* consulting the main leg's current PR lookup —
/// see `finish_release_protected` for why.
#[allow(clippy::too_many_arguments)]
fn finish_hotfix_protected(git: &dyn Git, hosting: &dyn HostingPlatform, cfg: &RepoConfig, major: u32, minor: u32, patch: u32, main_branch: &str, template: Option<&Path>, accept_merge_type: bool) -> Result<(), String> {
    let version = SemVer::new(major, minor, patch);
    let hotfix_branch = version.hotfix_branch();
    let tag = version.tag_name();

    let mut landed: Vec<LandedPr> = Vec::new();

    refuse_open_legacy_pr(hosting, &hotfix_branch, main_branch)?;
    let main_pr = finish_leg_landed(git, hosting, &hotfix_branch, main_branch, accept_merge_type)?;
    if let Some(pr) = &main_pr {
        landed.push(pr.clone());
    }

    let tag_landed = git.tag_exists(&tag)?
        && git.is_ancestor(&git.tag_commit_sha(&tag)?, &format!("origin/{main_branch}"))?;

    if tag_landed {
        push_tag_if_missing(git, &tag)?;
    } else {
        match &main_pr {
            Some(pr) => {
                tag_at_if_missing(git, &tag, &format!("chore: hotfix {version}"), &pr.merge_commit_sha)?;
                push_tag_if_missing(git, &tag)?;
            }
            None => {
                let title = format!("chore: merge hotfix {version} into {main_branch}");
                let finish = ensure_finish_branch(git, &hotfix_branch, main_branch, "gflow finish")?;
                let url = hosting.create_or_get_pr(&finish, main_branch, &title, landing_pr_body(template))?;
                announce_pending_landing(hosting, &title, &url, "gflow finish", &finish_conflict_hint(&finish, main_branch));
                return Ok(());
            }
        }
    }

    if let Some(pr) = &main_pr {
        report_commits_past_landing(git, &hotfix_branch, pr, main_branch, &tag)?;
    }

    let mut content_landed = false;
    let title = format!("chore: merge hotfix {version} into develop");
    match land_leg_strict(git, hosting, &hotfix_branch, "develop", &title, template, "gflow finish", accept_merge_type)? {
        LegState::Landed(pr) => landed.push(pr),
        LegState::ContentPresent => content_landed = true,
        LegState::Pending { url, finish } => {
            announce_pending_landing(hosting, &title, &url, "gflow finish", &finish_conflict_hint(&finish, "develop"));
            return Ok(());
        }
    }

    let mut release_branches = open_versioned_branches(git, hosting, cfg, main_branch, "release")?;
    release_branches.sort();

    for release in &release_branches {
        let title = format!("chore: merge hotfix {version} into {release}");
        match land_leg_strict(git, hosting, &hotfix_branch, release, &title, template, "gflow finish", accept_merge_type)? {
            LegState::Landed(pr) => landed.push(pr),
            LegState::ContentPresent => content_landed = true,
            LegState::Pending { url, finish } => {
                announce_pending_landing(hosting, &title, &url, "gflow finish", &finish_conflict_hint(&finish, release));
                return Ok(());
            }
        }
    }

    let mut finish_names = vec![
        finish_branch_name(&hotfix_branch, main_branch),
        finish_branch_name(&hotfix_branch, "develop"),
    ];
    finish_names.extend(release_branches.iter().map(|r| finish_branch_name(&hotfix_branch, r)));
    let tip_landed = content_landed || tip_landed_somewhere(git, &hotfix_branch, &landed, &finish_names)?;
    // Finish branches go first: source cleanup may remove the worktree the
    // process stands in, after which no git call can run.
    cleanup_finish_branches(git, &hotfix_branch)?;
    finish_hotfix_cleanup(git, cfg, &hotfix_branch, main_branch, &version, &release_branches, tip_landed)
}

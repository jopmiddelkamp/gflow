use std::path::Path;

use crate::flows::{
    announce_pending_landing, announce_version_pr, cleanup_finish_branches, delete_branch_guarded, delete_source_branch, ensure_finish_branch,
    finish_conflict_hint, finish_leg_landed, land_leg_strict, landing_pr_body, merge_where_checked_out, refuse_open_legacy_pr, LegState,
    merge_into, push_if_needed, push_tag_if_missing, report_commits_past_landing, require_clean_tree,
    resume_hint, run_version_script, tag_at_if_missing, tag_if_missing, tip_landed_somewhere,
};
use crate::git::Git;
use crate::hosting::{HostingPlatform, LandedPr, PrBody};
use crate::repo_config::{BumpStrategy, Mode, RepoConfig};
use crate::version::{finish_branch_name, SemVer};
use crate::version_script::VersionScript;

const NO_RC_TAG_ERROR: &str = "No RC tag found on this release branch. Run 'gflow bump' first.";
const NO_VERSION_TAG_ERROR: &str = "No version tag found on this release branch. Run 'gflow bump' first.";

/// The staging gate's catalog error: names the branch, the tag it must
/// catch up to, and the remedy. Shared by free mode's ancestor-guarded check
/// and protected mode's PR-guarded one — a squash-merged landing PR leaves no
/// ancestor relationship, so ancestor checks are invalid there; only this
/// message is common between them.
fn past_staged_tag_error(release_branch: &str, main_branch: &str, latest_tag: &str, commits_past: u32, strategy: BumpStrategy) -> String {
    let noun = if commits_past == 1 { "commit" } else { "commits" };
    let (deploy, next) = match strategy {
        BumpStrategy::Rc => ("an RC deploy", "the next RC"),
        BumpStrategy::Patch => ("a version deploy", "the next version"),
    };
    format!(
        "HEAD of {release_branch} is {commits_past} {noun} past {latest_tag}.\n\
         Every commit merged to {main_branch} must be validated on staging via {deploy}.\n\
         Run 'gflow bump' to cut {next}, wait for staging to pass, then 'gflow finish'."
    )
}

/// The branch's highest staging tag under `strategy` (`-rc.N` / clean patch),
/// with the strategy's own missing-tag catalog error.
fn latest_staged_tag(git: &dyn Git, branch: &str, major: u32, minor: u32, strategy: BumpStrategy) -> Result<SemVer, String> {
    match strategy {
        BumpStrategy::Rc => latest_rc(git, branch, major, minor)?.ok_or_else(|| NO_RC_TAG_ERROR.to_string()),
        BumpStrategy::Patch => latest_patch(git, branch, major, minor)?.ok_or_else(|| NO_VERSION_TAG_ERROR.to_string()),
    }
}

/// Highest `v{major}.{minor}.0-rc.N` tag on `branch`, or `None` when the branch
/// has no matching RC tag. Callers turn `None` into their own per-command error.
fn latest_rc(git: &dyn Git, branch: &str, major: u32, minor: u32) -> Result<Option<SemVer>, String> {
    let tags = git.tags_on_branch(branch)?;
    Ok(tags.iter().filter_map(|t| SemVer::parse(t))
        .filter(|v| v.major == major && v.minor == minor && v.patch == 0 && v.is_rc())
        .max())
}

/// Highest clean `v{major}.{minor}.Z` tag on `branch` (the patch strategy's
/// sibling of `latest_rc`), or `None` when the branch has none. Ancestor tags
/// from earlier releases carry a different major/minor, so the filter isolates
/// this release's own tags.
fn latest_patch(git: &dyn Git, branch: &str, major: u32, minor: u32) -> Result<Option<SemVer>, String> {
    let tags = git.tags_on_branch(branch)?;
    Ok(tags.iter().filter_map(|t| SemVer::parse(t))
        .filter(|v| v.major == major && v.minor == minor && !v.is_pre_release())
        .max())
}

/// The tag to cut next under `strategy`: one past the branch's highest existing
/// RC/patch tag, or the strategy's first tag (`-rc.1` / the clean release
/// itself) when it has none yet.
/// Reads `branch`'s tags, so callers call it only where a tag is actually about
/// to be cut or checked — protected mode's deferred/reuse paths never do.
fn next_version(git: &dyn Git, branch: &str, major: u32, minor: u32, release: &SemVer, strategy: BumpStrategy) -> Result<(Option<SemVer>, SemVer, String), String> {
    let (latest, next) = match strategy {
        BumpStrategy::Rc => {
            let latest = latest_rc(git, branch, major, minor)?;
            let next = latest.clone().map(|v| v.bump_rc()).unwrap_or_else(|| release.with_rc(1));
            (latest, next)
        }
        BumpStrategy::Patch => {
            let latest = latest_patch(git, branch, major, minor)?;
            let next = latest.clone().map(|v| v.bump_patch()).unwrap_or_else(|| release.clone());
            (latest, next)
        }
    };
    let tag = next.tag_name();
    Ok((latest, next, tag))
}

/// Announce the tag about to be cut: the ordinary "bumping from X" line, or —
/// when the branch has no tag of its strategy yet — the first-tag line instead.
fn announce_next_version(latest: Option<&SemVer>, next: &SemVer, tag: &str) {
    match latest {
        Some(l) => println!("Bumping version: {l} → {next}"),
        None if next.is_rc() => println!("Tagging first RC: {tag}"),
        None => println!("Tagging first version: {tag}"),
    }
}

/// Cut `tag` at the branch tip (the common case: nothing to wait on).
fn cut_tag_at_tip(git: &dyn Git, latest: Option<&SemVer>, next: &SemVer, tag: &str) -> Result<(), String> {
    announce_next_version(latest, next, tag);
    git.create_tag(tag, &format!("chore: bump version to {tag}"))?;
    git.push_tag(tag)?;
    println!("Tagged and pushed: {tag}");
    Ok(())
}

pub fn bump_version(
    git: &dyn Git,
    hosting: &dyn HostingPlatform,
    script: Option<&dyn VersionScript>,
    cfg: &RepoConfig,
    major: u32,
    minor: u32,
) -> Result<(), String> {
    let release = SemVer::new(major, minor, 0);
    let branch = release.release_branch();

    match cfg.mode {
        Mode::Free => bump_free(git, script, &release, &branch, major, minor, cfg.bump_strategy),
        Mode::Protected => bump_protected(git, hosting, script, &release, &branch, major, minor, cfg.bump_strategy),
    }
}

/// Free mode: no landing PR to wait for, so a version-script commit (if there
/// is a script) lands straight on the release branch and the tag is cut at the
/// tip in the same run. The script receives the version whose file state the
/// next tag must carry: the constant release under rc, the freshly incremented
/// patch under the patch strategy.
fn bump_free(git: &dyn Git, script: Option<&dyn VersionScript>, release: &SemVer, branch: &str, major: u32, minor: u32, strategy: BumpStrategy) -> Result<(), String> {
    let (latest, next, tag) = next_version(git, branch, major, minor, release, strategy)?;

    if let Some(script) = script {
        require_clean_tree(git)?;
        let script_version = match strategy {
            BumpStrategy::Rc => release,
            BumpStrategy::Patch => &next,
        };
        if run_version_script(git, script, script_version)? {
            git.push(branch)?;
        }
    }
    cut_tag_at_tip(git, latest.as_ref(), &next, &tag)
}

/// Protected mode: a version-script commit needs a human-merged PR before its
/// commit is trustworthy, so the RC tag is deferred to that PR's merge commit
/// rather than cut on a local commit. Order (task brief): consume a landed PR
/// first, then pure-tagging, then PR reuse, then a fresh script run.
#[allow(clippy::too_many_arguments)]
fn bump_protected(git: &dyn Git, hosting: &dyn HostingPlatform, script: Option<&dyn VersionScript>, release: &SemVer, branch: &str, major: u32, minor: u32, strategy: BumpStrategy) -> Result<(), String> {
    let chore_branch = release.release_chore_branch("set-version");

    if let Some(pr) = hosting.merged_pr_to(&chore_branch, branch)? {
        let (latest, next, tag) = next_version(git, branch, major, minor, release, strategy)?;
        let consumed = match &latest {
            Some(l) => git.tag_commit_sha(&l.tag_name())? == pr.merge_commit_sha,
            None => false,
        };
        if !consumed {
            announce_next_version(latest.as_ref(), &next, &tag);
            git.create_tag_at(&tag, &format!("chore: bump version to {tag}"), &pr.merge_commit_sha)?;
            git.push_tag(&tag)?;
            delete_branch_guarded(git, &chore_branch)?;
            println!("Tagged and pushed: {tag}");
            return Ok(());
        }
        // Already cut on a previous run — this run just tidies up remnants
        // (a chore branch the merge didn't delete) and re-evaluates as fresh.
        delete_branch_guarded(git, &chore_branch)?;
    }

    let Some(script) = script else {
        let (latest, next, tag) = next_version(git, branch, major, minor, release, strategy)?;
        return cut_tag_at_tip(git, latest.as_ref(), &next, &tag);
    };

    // The version the script must write — and thus the PR's title and commit
    // message. The patch strategy has to read the branch's tags to know it;
    // recomputing it on a later run converges (the next patch tag only ever
    // appears after this PR's merge is consumed).
    let script_version = match strategy {
        BumpStrategy::Rc => release.clone(),
        BumpStrategy::Patch => next_version(git, branch, major, minor, release, strategy)?.1,
    };
    let title = format!("chore: set version {script_version}");

    if git.remote_branch_exists(&chore_branch)? {
        let url = hosting.create_or_get_pr(&chore_branch, branch, &title, PrBody::NativeDefault)?;
        announce_deferred(hosting, &title, &url);
        return Ok(());
    }

    require_clean_tree(git)?;
    // A prior run can leave this branch behind locally (created, then
    // interrupted before the script committed or pushed). It is machine-owned,
    // so gflow clears it itself rather than dying on git's raw "branch already
    // exists" — remote-exists is already handled by the reuse path above.
    if git.local_branch_exists(&chore_branch)? {
        git.delete_branch_local(&chore_branch)?;
    }
    git.create_branch(&chore_branch, branch)?;
    match run_version_script(git, script, &script_version) {
        Ok(true) => {
            git.push(&chore_branch)?;
            let url = hosting.create_or_get_pr(&chore_branch, branch, &title, PrBody::NativeDefault)?;
            announce_deferred(hosting, &title, &url);
            git.checkout(branch)?;
            Ok(())
        }
        Ok(false) => {
            git.checkout(branch)?;
            git.delete_branch_local(&chore_branch)?;
            let (latest, next, tag) = next_version(git, branch, major, minor, release, strategy)?;
            cut_tag_at_tip(git, latest.as_ref(), &next, &tag)
        }
        Err(e) => {
            // Best-effort: don't strand the operator on the chore branch.
            let _ = git.checkout(branch);
            Err(e)
        }
    }
}

fn announce_deferred(hosting: &dyn HostingPlatform, title: &str, pr_url: &str) {
    announce_version_pr(hosting, title, pr_url);
    println!("The RC tag is deferred until this PR merges. After it merges, re-run 'gflow bump' to cut the tag.");
}

pub fn sync_with_develop(git: &dyn Git, hosting: &dyn HostingPlatform, cfg: &RepoConfig, major: u32, minor: u32, template: Option<&Path>, accept_merge_type: bool) -> Result<(), String> {
    let release = SemVer::new(major, minor, 0);
    let release_branch = release.release_branch();

    if cfg.mode == Mode::Protected {
        return sync_with_develop_protected(git, hosting, &release, &release_branch, template, accept_merge_type);
    }

    let current = git.current_branch()?;

    println!("Merging {release_branch} into develop...");
    git.checkout("develop")?;
    git.ff_merge("origin/develop")?;
    git.merge(&release_branch, &format!("chore: sync release {release} with develop"))?;
    git.push("develop")?;

    git.checkout(&current)?;
    println!("✔ Develop synced with {release_branch}.");

    Ok(())
}

/// Protected mode: gflow never merges into develop itself, so sync opens a
/// landing PR from the develop finish branch and stops for a human to merge.
/// The strict landed-check is what keeps a stale merged landing from being
/// trusted as "already synced" — a release with new commits since that merge
/// re-enters this same PR-opening path with a refreshed finish branch.
fn sync_with_develop_protected(git: &dyn Git, hosting: &dyn HostingPlatform, release: &SemVer, release_branch: &str, template: Option<&Path>, accept_merge_type: bool) -> Result<(), String> {
    let title = format!("chore: sync release {release} with develop");
    match land_leg_strict(git, hosting, release_branch, "develop", &title, template, "gflow sync", accept_merge_type)? {
        LegState::Landed(_) | LegState::ContentPresent => {
            println!("Develop already contains {release_branch}.");
            Ok(())
        }
        LegState::Pending { url, finish } => {
            announce_pending_landing(hosting, &title, &url, "gflow sync", &finish_conflict_hint(&finish, "develop"));
            Ok(())
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn finish_release(
    git: &dyn Git,
    hosting: &dyn HostingPlatform,
    cfg: &RepoConfig,
    major: u32,
    minor: u32,
    main_branch: &str,
    template: Option<&Path>,
    accept_merge_type: bool,
) -> Result<(), String> {
    if cfg.mode == Mode::Protected {
        return finish_release_protected(git, hosting, cfg, major, minor, main_branch, template, accept_merge_type);
    }

    let release = SemVer::new(major, minor, 0);
    let release_branch = release.release_branch();

    let latest_staged = latest_staged_tag(git, &release_branch, major, minor, cfg.bump_strategy)?;

    let release_version = latest_staged.to_release();
    let tag = release_version.tag_name();

    println!("Finishing release {release_branch} (tag: {tag})...");

    // Merge into the mainline — inline rather than merge_into(): the staging
    // gate must run inside the not-yet-merged branch, so a resume past that
    // merge never re-evaluates it.
    if !git.is_ancestor(&release_branch, main_branch)? {
        let latest_staged_tag = latest_staged.tag_name();
        let commits_past = git.rev_list_count(&latest_staged_tag, &release_branch)?;
        if commits_past > 0 {
            return Err(past_staged_tag_error(&release_branch, main_branch, &latest_staged_tag, commits_past, cfg.bump_strategy));
        }
        println!("Merging into {main_branch}...");
        merge_where_checked_out(git, &release_branch, main_branch, &format!("chore: merge release {release} into {main_branch}"))
            .map_err(|e| format!("{e}\n{}", resume_hint(&release_branch)))?;
    } else {
        println!("↷ skipped: merge into {main_branch} (already merged)");
    }

    // Patch strategy: the final tag is the last bump tag, already cut on the
    // release branch — nothing to tag at finish. The push stays in both
    // strategies: a bump's push_tag can have failed, and this is the last
    // moment the missing production tag gets caught before the branch goes.
    if cfg.bump_strategy == BumpStrategy::Rc {
        tag_if_missing(git, &tag, &format!("chore: release {release_version}"))?;
    }
    push_if_needed(git, main_branch)?;
    push_tag_if_missing(git, &tag)?;

    merge_into(git, &release_branch, "develop",
        &format!("chore: merge release {release} into develop"),
        &resume_hint(&release_branch))?;
    push_if_needed(git, "develop")?;

    finish_release_cleanup(git, cfg, &release_branch, main_branch, &release_version, true)
}

/// `tip_landed` gates deletion: free mode always passes `true` (its own
/// ancestor-based merge guard already proves the branch merged, so there is
/// no separate "did the tip go anywhere" question to ask).
pub fn finish_release_cleanup(git: &dyn Git, cfg: &RepoConfig, release_branch: &str, main_branch: &str, release_version: &SemVer, tip_landed: bool) -> Result<(), String> {
    println!("Cleaning up release branch...");
    if !tip_landed {
        eprintln!("⚠ Keeping {release_branch}: its tip is not part of any landed pull request, so deleting it could lose commits.");
        eprintln!("  Review it, then delete it yourself: git push origin --delete {release_branch}");
    } else if cfg.keep_release_branches {
        println!("Keeping {release_branch} (keep-release-branches=true).");
    } else {
        if let Some(path) = delete_source_branch(git, release_branch, main_branch)? {
            println!("Removed worktree: {}", path.display());
            println!("You can close this editor window now.");
        }
    }

    println!("✔ Release {release_version} complete.");
    Ok(())
}

/// Protected mode's staging gate: same rule and remedy as free mode's, but
/// reached only when there is no landed PR yet — a landed main PR already
/// proves every commit shipped through review, so this never re-runs after.
fn staging_gate(git: &dyn Git, release_branch: &str, main_branch: &str, major: u32, minor: u32, strategy: BumpStrategy) -> Result<(), String> {
    let latest = latest_staged_tag(git, release_branch, major, minor, strategy)?;
    let latest_tag = latest.tag_name();
    let commits_past = git.rev_list_count(&latest_tag, release_branch)?;
    if commits_past > 0 {
        return Err(past_staged_tag_error(release_branch, main_branch, &latest_tag, commits_past, strategy));
    }
    Ok(())
}

/// Protected mode: gflow never merges into `main`/`develop` itself (gflow
/// SKILL.md, "Landing modes"), so each landing step opens a PR and stops for a
/// human to merge; the next run picks up from wherever `leg_landed` finds it
/// landed.
/// The tag is checked for mainline containment *before* consulting the main
/// leg's current PR lookup — once a re-opened main PR has merged, that lookup
/// returns the new PR, whose merge commit is not what an already-cut tag
/// points at, and comparing against it would wrongly call a landed release
/// unlanded.
#[allow(clippy::too_many_arguments)]
fn finish_release_protected(git: &dyn Git, hosting: &dyn HostingPlatform, cfg: &RepoConfig, major: u32, minor: u32, main_branch: &str, template: Option<&Path>, accept_merge_type: bool) -> Result<(), String> {
    let release = SemVer::new(major, minor, 0);
    let release_branch = release.release_branch();

    let mut landed: Vec<LandedPr> = Vec::new();

    refuse_open_legacy_pr(hosting, &release_branch, main_branch)?;
    let main_pr = finish_leg_landed(git, hosting, &release_branch, main_branch, accept_merge_type)?;
    if let Some(pr) = &main_pr {
        landed.push(pr.clone());
    }

    // The version the finish ships: the clean release under rc, the last bump
    // tag under the patch strategy (refined once that tag is read below).
    let mut shipped_version = release.clone();

    match cfg.bump_strategy {
        BumpStrategy::Rc => {
            let tag = release.tag_name();
            let tag_landed = git.tag_exists(&tag)?
                && git.is_ancestor(&git.tag_commit_sha(&tag)?, &format!("origin/{main_branch}"))?;

            if tag_landed {
                // Landed and tagged on an earlier run. Still push: the tag can exist
                // locally and never have reached origin if a previous run stopped in
                // between.
                push_tag_if_missing(git, &tag)?;
            } else {
                match &main_pr {
                    Some(pr) => {
                        tag_at_if_missing(git, &tag, &format!("chore: release {release}"), &pr.merge_commit_sha)?;
                        push_tag_if_missing(git, &tag)?;
                    }
                    None => {
                        staging_gate(git, &release_branch, main_branch, major, minor, cfg.bump_strategy)?;
                        let title = format!("chore: merge release {release} into {main_branch}");
                        let finish = ensure_finish_branch(git, &release_branch, main_branch, "gflow finish")?;
                        let url = hosting.create_or_get_pr(&finish, main_branch, &title, landing_pr_body(template))?;
                        announce_pending_landing(hosting, &title, &url, "gflow finish", &finish_conflict_hint(&finish, main_branch));
                        return Ok(());
                    }
                }
            }

            if let Some(pr) = &main_pr {
                report_commits_past_landing(git, &release_branch, pr, main_branch, &tag)?;
            }
        }
        // Patch strategy: the final tag was cut and pushed at the last bump, on
        // the release branch itself — a squash landing's merge commit never
        // carries it, so there is nothing to tag, verify, or push here.
        BumpStrategy::Patch => match &main_pr {
            Some(pr) => {
                shipped_version = latest_patch(git, &release_branch, major, minor)?
                    .map(|v| v.to_release())
                    .unwrap_or(release.clone());
                // Same safety net as rc's tag_landed arm: the last bump's tag
                // can exist locally and never have reached origin.
                push_tag_if_missing(git, &shipped_version.tag_name())?;
                report_commits_past_landing(git, &release_branch, pr, main_branch, &shipped_version.tag_name())?;
            }
            None => {
                staging_gate(git, &release_branch, main_branch, major, minor, cfg.bump_strategy)?;
                let title = format!("chore: merge release {release} into {main_branch}");
                let finish = ensure_finish_branch(git, &release_branch, main_branch, "gflow finish")?;
                let url = hosting.create_or_get_pr(&finish, main_branch, &title, landing_pr_body(template))?;
                announce_pending_landing(hosting, &title, &url, "gflow finish", &finish_conflict_hint(&finish, main_branch));
                return Ok(());
            }
        },
    }

    let mut content_landed = false;
    let title = format!("chore: merge release {release} into develop");
    match land_leg_strict(git, hosting, &release_branch, "develop", &title, template, "gflow finish", accept_merge_type)? {
        LegState::Landed(pr) => landed.push(pr),
        LegState::ContentPresent => content_landed = true,
        LegState::Pending { url, finish } => {
            announce_pending_landing(hosting, &title, &url, "gflow finish", &finish_conflict_hint(&finish, "develop"));
            return Ok(());
        }
    }

    let finish_names = [
        finish_branch_name(&release_branch, main_branch),
        finish_branch_name(&release_branch, "develop"),
    ];
    let tip_landed = content_landed || tip_landed_somewhere(git, &release_branch, &landed, &finish_names)?;
    // Finish branches go first: source cleanup may remove the worktree the
    // process stands in, after which no git call can run.
    cleanup_finish_branches(git, &release_branch)?;
    finish_release_cleanup(git, cfg, &release_branch, main_branch, &shipped_version, tip_landed)
}

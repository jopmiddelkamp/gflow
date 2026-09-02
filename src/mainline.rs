//! Which branch is this repo's mainline: `main` or `master`.
//!
//! Resolved once in `lifecycle::run` and threaded into the flows as data, never
//! looked up mid-flow. The set is closed to those two names; widening it would
//! also mean widening `BranchType::parse`.

use crate::git::Git;

/// Written in **local** scope: the mainline belongs to the repository, not the
/// developer — the opposite default from `gflow.worktree.*`.
pub const MAIN_BRANCH_KEY: &str = "gflow.branch.main";

const SUPPORTED: [&str; 2] = ["main", "master"];

/// Persists the detected value, so later runs are a single config read.
pub fn resolve_main_branch(git: &dyn Git) -> Result<String, String> {
    if let Some(configured) = git.get_config(MAIN_BRANCH_KEY)? {
        let configured = configured.trim();
        if !configured.is_empty() {
            if !SUPPORTED.contains(&configured) {
                return Err(format!(
                    "Unsupported mainline branch '{configured}' in {MAIN_BRANCH_KEY}. \
                     gflow supports 'main' or 'master'. \
                     Fix it with 'git config {MAIN_BRANCH_KEY} main'."
                ));
            }
            return Ok(configured.to_string());
        }
    }

    let detected = detect(git)?;
    git.set_config(MAIN_BRANCH_KEY, &detected, false)?;
    println!("Detected mainline branch '{detected}' (saved to {MAIN_BRANCH_KEY}).");
    Ok(detected)
}

/// A repo with neither branch is about to create one; `main` is the default.
fn detect(git: &dyn Git) -> Result<String, String> {
    for candidate in SUPPORTED {
        if git.local_branch_exists(candidate)? || git.remote_branch_exists(candidate)? {
            return Ok(candidate.to_string());
        }
    }
    Ok("main".to_string())
}

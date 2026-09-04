//! First-run initialisation: the interactive wizard that writes `.gflow/config`,
//! and the "present / missing" decision that gates every other command on it.

use std::path::Path;

use crate::prompt::Prompter;
use crate::repo_config::{self, BumpStrategy, Mode, RepoConfig, NOT_INITIALISED};

const ALREADY: &str = "Already initialised: edit .gflow/config directly (mode, keep-release-branches, bump-strategy).";

/// Load the repo policy, running the wizard first when the repo has none and
/// the run is interactive. Non-interactive runs (subcommands, CI) refuse.
pub fn ensure(prompter: &dyn Prompter, repo_root: &Path, interactive: bool) -> Result<RepoConfig, String> {
    if repo_config::exists(repo_root) {
        return repo_config::load(repo_root);
    }
    if interactive {
        println!("This repository is not initialised for gflow yet.\n");
        return wizard(prompter, repo_root);
    }
    Err(NOT_INITIALISED.to_string())
}

/// `gflow init`.
pub fn run(prompter: &dyn Prompter, repo_root: &Path) -> Result<(), String> {
    if repo_config::exists(repo_root) {
        return Err(ALREADY.to_string());
    }
    wizard(prompter, repo_root).map(|_| ())
}

/// Ask the three policy questions (defaults first), write `.gflow/config`.
pub fn wizard(prompter: &dyn Prompter, repo_root: &Path) -> Result<RepoConfig, String> {
    let mode = match prompter.select("Landing mode", &[
        "free — merge and push directly",
        "protected — every landing goes through a PR",
    ])? {
        0 => Mode::Free,
        _ => Mode::Protected,
    };
    let keep_release_branches = prompter.select("Release branches after finish", &["delete (default)", "keep"])? == 1;
    let bump_strategy = match prompter.select("Bump strategy", &[
        "rc — pre-release tags, one clean tag at finish (default)",
        "patch — real patch version on every bump",
    ])? {
        0 => BumpStrategy::Rc,
        _ => BumpStrategy::Patch,
    };
    let cfg = RepoConfig { mode, keep_release_branches, bump_strategy };
    repo_config::write(repo_root, &cfg)?;
    println!("\nWrote .gflow/config — commit it so every clone (and CI) shares this policy: git add .gflow/config && git commit -m \"chore: initialise gflow\"");
    Ok(cfg)
}

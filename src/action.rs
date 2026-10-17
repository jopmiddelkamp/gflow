//! The single currency both interfaces resolve into: the menu (`menu.rs`) and
//! the CLI subcommands (`cli.rs`) each produce an `Action`, and nothing
//! downstream knows which interface ran. Lives in its own module so neither
//! interface has to import the other.

use crate::flows::start::ReleaseType;

#[derive(Debug, PartialEq)]
pub enum Action {
    StartWorkBranch { prefix: String, name: String, from: String, no_checkout: bool, no_worktree: bool },
    StartRelease { release_type: Option<ReleaseType>, no_worktree: bool },
    StartReleaseFix { name: String, no_checkout: bool, no_worktree: bool },
    StartHotfixFix { name: String, no_checkout: bool, no_worktree: bool },
    FinishWorkBranch { breaking: Option<bool>, base: Option<String> },
    FinishReleaseFix,
    FinishRelease,
    FinishHotfix,
    FinishHotfixFix,
    AbortFinish,
    BumpVersion,
    SyncWithDevelop,
}

impl Action {
    pub fn is_start(&self) -> bool {
        matches!(
            self,
            Action::StartWorkBranch { .. }
                | Action::StartRelease { .. }
                | Action::StartReleaseFix { .. }
                | Action::StartHotfixFix { .. }
        )
    }

    pub fn no_checkout(&self) -> bool {
        match self {
            Action::StartWorkBranch { no_checkout, .. } => *no_checkout,
            Action::StartReleaseFix { no_checkout, .. } => *no_checkout,
            Action::StartHotfixFix { no_checkout, .. } => *no_checkout,
            _ => false,
        }
    }

    pub fn no_worktree(&self) -> bool {
        match self {
            Action::StartWorkBranch { no_worktree, .. } => *no_worktree,
            Action::StartRelease { no_worktree, .. } => *no_worktree,
            Action::StartReleaseFix { no_worktree, .. } => *no_worktree,
            Action::StartHotfixFix { no_worktree, .. } => *no_worktree,
            _ => false,
        }
    }
}

/// Branch-name validation shared by the CLI (`--name`) and the interactive
/// prompt — same rules, same message, whichever interface ran.
pub fn validate_branch_name(input: &str) -> Result<(), String> {
    if input.is_empty() {
        return Err("Name cannot be empty".to_string());
    }
    if input.contains("..") || input.contains('~') || input.contains('^') || input.contains(':') || input.contains('\\') {
        return Err("Invalid branch name. Avoid special characters (.. ~ ^ : \\)".to_string());
    }
    Ok(())
}

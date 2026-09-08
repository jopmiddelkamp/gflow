use clap::{Args, Subcommand};
use crate::git::branch::BranchType;
use crate::flows::start::ReleaseType;
use crate::action::{validate_branch_name, Action};

#[derive(Subcommand)]
pub enum Commands {
    /// Start a new branch
    Start {
        #[command(subcommand)]
        kind: StartKind,
    },
    /// Finish the current branch (infers action from branch type).
    /// If an in-progress release/hotfix finish was interrupted (e.g. by a merge
    /// conflict), re-running this command resumes from the first incomplete step.
    Finish {
        /// Mark PR as containing breaking changes (adds ! to commit type).
        /// Omit to prompt interactively. Use --breaking (true) or --breaking=false
        /// to skip the prompt non-interactively.
        #[arg(long, num_args = 0..=1, default_missing_value = "true")]
        breaking: Option<bool>,
        /// PR target branch (work branches only). Skips parent-branch detection
        /// and its selection menu — required for non-interactive use when more
        /// than one candidate parent exists.
        #[arg(long, conflicts_with = "abort")]
        base: Option<String>,
        /// Discard any in-progress finish state without running the flow.
        #[arg(long, conflicts_with = "breaking")]
        abort: bool,
        /// Accept a PR that was completed with the wrong type (squash vs merge
        /// commit) and continue instead of stopping with undo instructions.
        #[arg(long)]
        accept_merge_type: bool,
    },
    /// Bump the patch version on the current release branch
    Bump,
    /// Sync the current release branch into develop
    Sync {
        /// Accept a landing PR that was completed with the wrong type (squash
        /// instead of merge commit) and continue.
        #[arg(long)]
        accept_merge_type: bool,
    },
    /// Initialise this repository for gflow: writes .gflow/config (interactive)
    Init,
    /// Configure the optional worktree flow (run with no subcommand for an interactive setup)
    Worktree {
        #[command(subcommand)]
        action: Option<WorktreeAction>,
        /// Write to this repository's committed .gflow/config (shared with everyone who clones it)
        #[arg(long, global = true, conflicts_with = "local")]
        repo: bool,
        /// Write to this repository's .gflow/config.local (yours only, never committed)
        #[arg(long, global = true)]
        local: bool,
    },
}

#[derive(Subcommand)]
pub enum WorktreeAction {
    /// Turn the worktree flow on
    Enable,
    /// Turn the worktree flow off
    Disable,
    /// Set the editor command opened for each worktree (e.g. code, cursor, none)
    Editor {
        value: String,
    },
    /// Set the base directory worktree folders are created in
    Path {
        value: String,
    },
    /// Show the current worktree configuration
    Status,
}

#[derive(Args, Debug, Clone, Default)]
pub struct StartOptions {
    /// Create and push the branch without checking it out
    #[arg(long)]
    pub no_checkout: bool,
    /// Skip the worktree flow for this command (when gflow.worktree.enabled is set)
    #[arg(long)]
    pub no_worktree: bool,
}

#[derive(Subcommand)]
pub enum StartKind {
    /// Start a new feature branch
    Feature {
        #[arg(long)]
        name: String,
        #[arg(long, default_value = "develop")]
        base: String,
        #[command(flatten)]
        opts: StartOptions,
    },
    /// Start a new fix branch
    Fix {
        #[arg(long)]
        name: String,
        #[arg(long, default_value = "develop")]
        base: String,
        #[command(flatten)]
        opts: StartOptions,
    },
    /// Start a new chore branch
    Chore {
        #[arg(long)]
        name: String,
        #[arg(long, default_value = "develop")]
        base: String,
        #[command(flatten)]
        opts: StartOptions,
    },
    /// Start a new docs branch
    Docs {
        #[arg(long)]
        name: String,
        #[arg(long, default_value = "develop")]
        base: String,
        #[command(flatten)]
        opts: StartOptions,
    },
    /// Start a new refactor branch
    Refactor {
        #[arg(long)]
        name: String,
        #[arg(long, default_value = "develop")]
        base: String,
        #[command(flatten)]
        opts: StartOptions,
    },
    /// Start a new release branch (or resume existing)
    Release {
        #[arg(long, conflicts_with = "minor")]
        major: bool,
        #[arg(long, conflicts_with = "major")]
        minor: bool,
        /// Skip the worktree flow for this command (no effect unless gflow.worktree.enabled)
        #[arg(long)]
        no_worktree: bool,
    },
    /// Start a release fix branch (must be on a release branch)
    ReleaseFix {
        #[arg(long)]
        name: String,
        #[command(flatten)]
        opts: StartOptions,
    },
    /// Start a hotfix fix branch (must be on the mainline or a hotfix branch)
    HotfixFix {
        #[arg(long)]
        name: String,
        #[command(flatten)]
        opts: StartOptions,
    },
}

fn start_work_branch(prefix: &str, name: String, base: String, no_checkout: bool, no_worktree: bool) -> Result<Action, String> {
    validate_branch_name(&name)?;
    Ok(Action::StartWorkBranch { prefix: prefix.to_string(), name, from: base, no_checkout, no_worktree })
}

/// True when the flow will discover the target release/hotfix branch itself
/// instead of requiring the user to be standing on it: either `--no-checkout`
/// was passed, or the worktree flow is enabled and not opted out of.
fn auto_discovers_target(opts: &StartOptions, worktree_enabled: bool) -> bool {
    opts.no_checkout || (worktree_enabled && !opts.no_worktree)
}

fn require_release_branch(branch_type: &BranchType) -> Result<(), String> {
    if !matches!(branch_type, BranchType::Release { .. }) {
        return Err("This command is only valid on a release branch.".to_string());
    }
    Ok(())
}

/// Resolve the parsed command into an Action.
///
/// `worktree_enabled` is the `gflow.worktree.enabled` config value. Like
/// `--no-checkout`, an active worktree flow auto-discovers the target branch for
/// release-fix/hotfix-fix, so the "must be standing on it" branch-type gate only
/// applies to the plain checkout path.
pub fn resolve_action(command: Commands, branch_type: &BranchType, worktree_enabled: bool, main_branch: &str) -> Result<Action, String> {
    match command {
        Commands::Start { kind } => match kind {
            StartKind::Feature { name, base, opts } => start_work_branch("feature", name, base, opts.no_checkout, opts.no_worktree),
            StartKind::Fix { name, base, opts } => start_work_branch("fix", name, base, opts.no_checkout, opts.no_worktree),
            StartKind::Chore { name, base, opts } => start_work_branch("chore", name, base, opts.no_checkout, opts.no_worktree),
            StartKind::Docs { name, base, opts } => start_work_branch("docs", name, base, opts.no_checkout, opts.no_worktree),
            StartKind::Refactor { name, base, opts } => start_work_branch("refactor", name, base, opts.no_checkout, opts.no_worktree),
            StartKind::Release { major, minor, no_worktree } => {
                let release_type = if major { Some(ReleaseType::Major) }
                    else if minor { Some(ReleaseType::Minor) }
                    else { None };
                Ok(Action::StartRelease { release_type, no_worktree })
            }
            StartKind::ReleaseFix { name, opts } => {
                validate_branch_name(&name)?;
                if !auto_discovers_target(&opts, worktree_enabled) {
                    require_release_branch(branch_type)?;
                }
                Ok(Action::StartReleaseFix { name, no_checkout: opts.no_checkout, no_worktree: opts.no_worktree })
            }
            StartKind::HotfixFix { name, opts } => {
                validate_branch_name(&name)?;
                if !auto_discovers_target(&opts, worktree_enabled)
                    && !matches!(branch_type, BranchType::Main | BranchType::Hotfix { .. })
                {
                    return Err(format!("This command is only valid on a {main_branch} or hotfix branch."));
                }
                Ok(Action::StartHotfixFix { name, no_checkout: opts.no_checkout, no_worktree: opts.no_worktree })
            }
        },
        Commands::Finish { breaking, base, abort, .. } => {
            if abort {
                unreachable!("--abort is intercepted in lifecycle::resolve_action_with_state before dispatch")
            }
            if base.is_some() && branch_type.has_fixed_finish_target() {
                return Err("--base is only supported when finishing a work branch (feature/fix/chore/docs/refactor); this branch type has a fixed target.".to_string());
            }
            match branch_type {
                BranchType::Main | BranchType::Develop => {
                    Err("Nothing to finish on this branch.".to_string())
                }
                BranchType::Feature { .. } | BranchType::Fix { .. } | BranchType::Chore { .. }
                | BranchType::Docs { .. } | BranchType::Refactor { .. } => {
                    Ok(Action::FinishWorkBranch { breaking, base })
                }
                BranchType::Release { .. } => Ok(Action::FinishRelease),
                BranchType::ReleaseFix { .. } => Ok(Action::FinishReleaseFix),
                BranchType::ReleaseChore { .. } => Ok(Action::FinishReleaseChore),
                BranchType::Hotfix { .. } => Ok(Action::FinishHotfix),
                BranchType::HotfixFix { .. } => Ok(Action::FinishHotfixFix),
                BranchType::Other => {
                    Err("Not on a recognized gitflow branch.".to_string())
                }
            }
        }
        Commands::Bump => {
            require_release_branch(branch_type)?;
            Ok(Action::BumpVersion)
        }
        Commands::Sync { .. } => {
            require_release_branch(branch_type)?;
            Ok(Action::SyncWithDevelop)
        }
        Commands::Worktree { .. } => {
            unreachable!("worktree configuration is dispatched in main() before resolve_action")
        }
        Commands::Init => {
            unreachable!("init is dispatched in main() before resolve_action")
        }
    }
}

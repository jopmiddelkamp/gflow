use std::process::{Command, ExitCode};

use clap::Parser;

use gflow::cli::{Commands, WorktreeAction};
use gflow::git::{GitCli, SystemRunner};
use gflow::git::Git;
use gflow::hosting::detect::{self, Provider};
use gflow::hosting::devops::AzureDevOps;
use gflow::hosting::github::GitHub;
use gflow::hosting::{HostingPlatform, SystemCli};
use gflow::lifecycle;
use gflow::menu::MenuPrompter;
use gflow::editor::CommandEditor;
use gflow::init;
use gflow::version_script::{self, ScriptCli, VersionScript};
use gflow::worktree::{self, WorktreeConfig, WorktreeEnv};
use gflow::worktree_setup::{self, ShellSetup};

#[derive(Parser)]
#[command(name = "gflow", version, about = "gflow - a customized gitflow workflow CLI")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    if let Err(e) = run(cli.command) {
        eprintln!("Error: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// Composition root: build adapters, run preflight, hand off to the lifecycle
/// (which lives in the library so its crash-safety ordering is testable).
fn run(command: Option<Commands>) -> Result<(), String> {
    check_command_exists("git")?;
    let git = GitCli::new(&SystemRunner);

    // `gflow worktree` only reads/writes git config — no gh, auth, fetch, or branch
    // context needed. Dispatch it here and return before the branch-flow machinery.
    let command = match command {
        Some(Commands::Worktree { action, local }) => {
            return run_worktree_config(&git, action, local);
        }
        other => other,
    };

    // Provider detection reads the origin remote, so the repo check comes first.
    git.current_branch().map_err(|_| "Not in a git repository.".to_string())?;

    // Eager resolve: a platform-mismatched committed script errors on every
    // command, not just release/hotfix ones. Accepted — it surfaces the
    // misconfiguration immediately rather than on whichever command hits it first.
    let root = git.worktree_root()?;
    if let Some(Commands::Init) = command {
        return init::run(&MenuPrompter, &root);
    }
    let repo_cfg = init::ensure(&MenuPrompter, &root, command.is_none())?;
    let script_path = version_script::resolve(&root)?;
    let script = script_path.map(|path| ScriptCli::new(path, root.clone()));

    let hosting = create_hosting(&git)?;
    let wt_config = WorktreeConfig::load(&git)?;
    let editor = CommandEditor::new(wt_config.editor.clone());
    let (setup_commands, setup_warning) = worktree_setup::resolve(&root, wt_config.enabled);
    if let Some(warning) = setup_warning {
        eprintln!("Warning: {warning}");
    }
    let worktree_env = WorktreeEnv {
        config: &wt_config,
        editor: &editor,
        setup: &ShellSetup,
        commands: setup_commands.as_ref(),
    };
    let prompter = MenuPrompter;

    lifecycle::run(
        &git,
        &*hosting,
        &prompter,
        &worktree_env,
        &repo_cfg,
        script.as_ref().map(|s| s as &dyn VersionScript),
        command,
    )
}

/// Detect the hosting provider for this repo and return a ready-to-use,
/// preflighted (CLI installed + authenticated) hosting backend.
fn create_hosting(git: &dyn Git) -> Result<Box<dyn HostingPlatform>, String> {
    match detect::detect(git)? {
        Provider::GitHub => {
            check_command_exists("gh")?;
            let hosting = GitHub::new(&SystemCli);
            hosting.check_auth().map_err(|e| {
                format!("GitHub CLI is not authenticated. Run 'gh auth login' first.\n{e}")
            })?;
            Ok(Box::new(hosting))
        }
        Provider::AzureDevOps { org, project, repo } => {
            check_command_exists("az")?;
            let hosting = AzureDevOps::new(org, project, repo, &SystemCli);
            hosting.check_auth().map_err(|e| {
                format!("Azure CLI is not ready for Azure DevOps. Run 'az login' (or 'az devops login' with a PAT).\n{e}")
            })?;
            Ok(Box::new(hosting))
        }
    }
}

fn run_worktree_config(git: &GitCli<'_>, action: Option<WorktreeAction>, local: bool) -> Result<(), String> {
    match action {
        None => worktree::wizard(git, &MenuPrompter, local),
        Some(WorktreeAction::Enable) => worktree::set_enabled(git, true, local),
        Some(WorktreeAction::Disable) => worktree::set_enabled(git, false, local),
        Some(WorktreeAction::Editor { value }) => worktree::set_editor(git, &value, local),
        Some(WorktreeAction::Path { value }) => worktree::set_path(git, &value, local),
        Some(WorktreeAction::Status) => worktree::show_status(git),
    }
}

fn check_command_exists(cmd: &str) -> Result<(), String> {
    Command::new(cmd)
        .arg("--version")
        .output()
        .map_err(|_| format!("'{cmd}' is not installed or not in PATH."))?;
    Ok(())
}

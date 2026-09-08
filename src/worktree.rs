use std::path::{Path, PathBuf};

use crate::editor::Editor;
use crate::worktree_setup::{SetupCommands, WorktreeSetup};
use crate::git::{Git, Result};
use crate::prompt::Prompter;
use crate::repo_config::{self, Settings};

/// Friendly editor names offered by the interactive wizard, mapped to the launcher
/// command gflow stores and runs. Any editor with a `<cmd> <path>` CLI works — this
/// is just convenience; `gflow worktree editor <cmd>` accepts any command.
pub const EDITOR_PRESETS: &[(&str, &str)] = &[
    ("VS Code", "code"),
    ("Cursor", "cursor"),
    ("Windsurf", "windsurf"),
    ("Zed", "zed"),
    ("IntelliJ IDEA", "idea"),
    ("PyCharm", "pycharm"),
    ("WebStorm", "webstorm"),
];

/// Which of the three config files a `gflow worktree` write lands in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConfigScope {
    /// `~/.gflow/config` — you, in every repository.
    Global,
    /// `<repo>/.gflow/config` — this repository, committed, shared with everyone.
    Repo,
    /// `<repo>/.gflow/config.local` — you, in this repository, never committed.
    Local,
}

impl ConfigScope {
    /// `--repo` and `--local` are mutually exclusive at the CLI, so the flags
    /// can never both be set; global is the default.
    pub fn from_flags(repo: bool, local: bool) -> Self {
        match (repo, local) {
            (true, _) => ConfigScope::Repo,
            (_, true) => ConfigScope::Local,
            _ => ConfigScope::Global,
        }
    }

    fn label(self) -> &'static str {
        match self {
            ConfigScope::Global => "~/.gflow/config (all repos)",
            ConfigScope::Repo => ".gflow/config (this repo, committed)",
            ConfigScope::Local => ".gflow/config.local (this repo, not committed)",
        }
    }
}

/// An editor value that is blank or `none` (any case) means "don't open an editor".
fn editor_disabled(editor: &str) -> bool {
    let editor = editor.trim();
    editor.is_empty() || editor.eq_ignore_ascii_case("none")
}

/// User configuration for the optional worktree flow, taken from the resolved
/// `worktree` / `editor` / `path` keys of the config layer stack.
#[derive(Debug)]
pub struct WorktreeConfig {
    pub enabled: bool,
    pub editor: String,
    pub base_path: Option<String>,
}

impl WorktreeConfig {
    /// Absent keys fall back to defaults (disabled, editor `code`, no custom
    /// base path). Values are trimmed — stray whitespace would otherwise break
    /// `Command::new` (e.g. editor `"code "`) or produce oddly named directories.
    pub fn from_settings(settings: &Settings) -> Self {
        let trimmed = |v: &Option<String>| {
            v.as_ref().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
        };
        Self {
            enabled: settings.worktree.unwrap_or(false),
            editor: trimmed(&settings.editor).unwrap_or_else(|| "code".to_string()),
            base_path: trimmed(&settings.path),
        }
    }
}

/// Everything the worktree flow needs from the composition root: config,
/// editor, the setup-command runner and the repo's resolved setup commands.
pub struct WorktreeEnv<'a> {
    pub config: &'a WorktreeConfig,
    pub editor: &'a dyn Editor,
    pub setup: &'a dyn WorktreeSetup,
    pub commands: Option<&'a SetupCommands>,
}

/// Per-run handle passed into the start flows when the worktree flow is active.
pub struct WorktreeContext<'a> {
    pub env: &'a WorktreeEnv<'a>,
    pub prompter: &'a dyn Prompter,
}

/// Expand a leading `~` / `~/` to the user's home directory. The shell never
/// sees git config values, so without this a configured `~/worktrees` would
/// create a literal `~` directory. Falls back to the input verbatim when no
/// home directory can be determined (`~user` forms are not supported).
fn expand_tilde(path: &str) -> PathBuf {
    if path == "~" || path.starts_with("~/") {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .filter(|v| !v.is_empty());
        if let Some(home) = home {
            let mut expanded = PathBuf::from(home);
            if let Some(rest) = path.strip_prefix("~/") {
                expanded.push(rest);
            }
            return expanded;
        }
    }
    PathBuf::from(path)
}

/// Compute the worktree directory for `branch`.
///
/// Folder name is `<repo-name>-<branch-with-slashes-as-dashes>`, placed in
/// `base_path` if set (a leading `~` is expanded to the home directory),
/// otherwise the repository's parent directory.
pub fn worktree_path(repo_root: &Path, repo_name: &str, base_path: Option<&str>, branch: &str) -> PathBuf {
    let folder = format!("{repo_name}-{}", branch.replace('/', "-"));
    let base: PathBuf = match base_path {
        Some(p) => expand_tilde(p),
        None => repo_root
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(".")),
    };
    base.join(folder)
}

/// Create a git worktree for `branch` and open it in the configured editor.
///
/// `branch` must already exist. Worktree creation is fatal on error; editor-open
/// failures are downgraded to a warning since the branch and worktree already exist.
pub fn open_worktree(git: &dyn Git, ctx: &WorktreeContext<'_>, branch: &str) -> Result<()> {
    let config = ctx.env.config;
    let editor = ctx.env.editor;
    let repo_root = git.repo_root()?;
    let repo_name = repo_root
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("could not determine repository name from its path")?;
    let path = worktree_path(&repo_root, repo_name, config.base_path.as_deref(), branch);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create worktree base directory '{}': {e}", parent.display()))?;
    }

    println!("Creating worktree: {}", path.display());
    git.add_worktree(&path, branch)?;

    if let Some(cmds) = ctx.env.commands {
        run_setup(ctx, &repo_root, &path, cmds);
    }

    if !editor_disabled(&config.editor) {
        let editor_cmd = config.editor.trim();
        println!("Opening in editor: {editor_cmd}");
        if let Err(e) = editor.open(&path) {
            eprintln!("Warning: {e}. Worktree is ready at {}.", path.display());
        }
    }

    Ok(())
}

/// Run the repo's `worktrees.json` commands inside the new worktree. Never
/// fails the start: a failing command is reported and the rest still run
/// (worktree-cli's policy) — the branch is already pushed by now.
fn run_setup(ctx: &WorktreeContext<'_>, main_root: &Path, worktree: &Path, cmds: &SetupCommands) {
    println!("Running {} setup command(s) from {}", cmds.commands.len(), cmds.file.display());
    let mut ok = 0;
    for command in &cmds.commands {
        println!("Running: {command}");
        match ctx.env.setup.run_command(worktree, main_root, command) {
            Ok(()) => ok += 1,
            Err(e) => eprintln!("Setup command failed: {command} — {e}"),
        }
    }
    println!("Setup commands completed ({ok}/{} succeeded).", cmds.commands.len());
}

// ---------------------------------------------------------------------------
// `gflow worktree` configuration commands
// ---------------------------------------------------------------------------

/// `gflow worktree`: read the effective settings for `status`, otherwise write
/// the chosen key to the file the scope selects. Lives here rather than in the
/// composition root so the scope-to-file decision stays mock-tested.
pub fn run_config(
    prompter: &dyn Prompter,
    repo_root: Option<&Path>,
    home: Option<&Path>,
    action: Option<crate::cli::WorktreeAction>,
    scope: ConfigScope,
) -> Result<()> {
    use crate::cli::WorktreeAction;
    if let Some(WorktreeAction::Status) = action {
        let layers = repo_config::load_layers(home, repo_root)?;
        for warning in &layers.warnings {
            eprintln!("Warning: {warning}");
        }
        show_status(&WorktreeConfig::from_settings(&layers.settings));
        return Ok(());
    }
    let target = config_target(repo_root, home, scope)?;
    if scope == ConfigScope::Local {
        repo_config::ensure_local_gitignored(
            target.parent().expect("config target always has a parent"),
        )?;
    }
    match action {
        None => wizard(&target, prompter, scope),
        Some(WorktreeAction::Enable) => set_enabled(&target, true, scope),
        Some(WorktreeAction::Disable) => set_enabled(&target, false, scope),
        Some(WorktreeAction::Editor { value }) => set_editor(&target, &value, scope),
        Some(WorktreeAction::Path { value }) => set_path(&target, &value, scope),
        Some(WorktreeAction::Status) => unreachable!("status returns above"),
    }
}

/// Which config file `gflow worktree` writes, for the scope the flags selected.
pub fn config_target(
    repo_root: Option<&Path>,
    home: Option<&Path>,
    scope: ConfigScope,
) -> Result<PathBuf> {
    let in_repo = |flag: &str| {
        repo_root.ok_or_else(|| format!("Not in a git repository — '{flag}' needs one."))
    };
    match scope {
        ConfigScope::Repo => Ok(repo_config::config_path(in_repo("--repo")?)),
        ConfigScope::Local => Ok(repo_config::local_config_path(in_repo("--local")?)),
        ConfigScope::Global => {
            let home = home.ok_or(
                "Cannot determine your home directory (HOME/USERPROFILE), so there is nowhere to save settings for all repositories. Use '--repo' or '--local'.",
            )?;
            Ok(repo_config::global_config_path(home))
        }
    }
}

/// Turn the worktree flow on or off.
pub fn set_enabled(target: &Path, enabled: bool, scope: ConfigScope) -> Result<()> {
    repo_config::set_key(target, "worktree", Some(if enabled { "true" } else { "false" }))?;
    println!(
        "Worktree flow {} — saved to {}.",
        if enabled { "enabled" } else { "disabled" },
        scope.label(),
    );
    Ok(())
}

/// Set the editor command opened for each worktree.
pub fn set_editor(target: &Path, value: &str, scope: ConfigScope) -> Result<()> {
    repo_config::set_key(target, "editor", Some(value))?;
    println!("Worktree editor set to '{value}' — saved to {}.", scope.label());
    Ok(())
}

/// Set the base directory worktree folders are created in.
pub fn set_path(target: &Path, value: &str, scope: ConfigScope) -> Result<()> {
    repo_config::set_key(target, "path", Some(value))?;
    println!("Worktree base directory set to '{value}' — saved to {}.", scope.label());
    Ok(())
}

/// Clear a custom base directory, reverting to the default (the repo's parent).
pub fn use_default_path(target: &Path, scope: ConfigScope) -> Result<()> {
    repo_config::set_key(target, "path", None)?;
    println!("Worktree base directory reset to the default (the repo's parent) in {}.", scope.label());
    Ok(())
}

/// Print the effective worktree configuration.
pub fn show_status(cfg: &WorktreeConfig) {
    println!("Worktree flow configuration");
    println!("  enabled : {}", cfg.enabled);
    if editor_disabled(&cfg.editor) {
        println!("  editor  : {} (won't open an editor)", cfg.editor);
    } else {
        println!("  editor  : {}", cfg.editor);
    }
    match &cfg.base_path {
        Some(p) => println!("  path    : {p}"),
        None => println!("  path    : (default — the repository's parent directory)"),
    }
    if !cfg.enabled {
        println!("\nIt's off. Turn it on with 'gflow worktree enable' or 'gflow worktree'.");
    }
}

/// Interactive setup: prompts for enable, editor, and location, then saves them.
pub fn wizard(target: &Path, prompter: &dyn Prompter, scope: ConfigScope) -> Result<()> {
    println!("Configure the worktree flow — writing to {}.\n", scope.label());

    let enable_items = [
        "Enable — open each new branch in its own worktree + editor",
        "Disable",
    ];
    let enabled = prompter.select("Worktree flow", &enable_items)? == 0;
    set_enabled(target, enabled, scope)?;
    if !enabled {
        return Ok(());
    }

    // Editor: presets, then None, then Custom.
    let mut editor_items: Vec<String> = EDITOR_PRESETS
        .iter()
        .map(|(label, cmd)| format!("{label}  ({cmd})"))
        .collect();
    editor_items.push("None — create the worktree but don't open an editor".to_string());
    editor_items.push("Custom command…".to_string());
    let editor_refs: Vec<&str> = editor_items.iter().map(String::as_str).collect();
    let e_idx = prompter.select("Editor", &editor_refs)?;
    let editor_value = if e_idx < EDITOR_PRESETS.len() {
        EDITOR_PRESETS[e_idx].1.to_string()
    } else if e_idx == EDITOR_PRESETS.len() {
        "none".to_string()
    } else {
        prompter.prompt_line("Editor command (e.g. code, cursor)")?
    };
    set_editor(target, &editor_value, scope)?;

    // Location: default (repo's parent) or a custom directory.
    let path_items = ["Default — next to the repository", "Custom directory…"];
    if prompter.select("Worktree location", &path_items)? == 0 {
        use_default_path(target, scope)?;
    } else {
        let path = prompter.prompt_line("Worktree base directory (e.g. ~/worktrees)")?;
        set_path(target, &path, scope)?;
    }

    println!("\nDone. Your next 'gflow start' opens work in a worktree.");
    Ok(())
}

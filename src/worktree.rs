use std::path::{Path, PathBuf};

use crate::editor::Editor;
use crate::worktree_setup::{SetupCommands, WorktreeSetup};
use crate::git::{Git, Result};
use crate::prompt::Prompter;

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

const KEY_ENABLED: &str = "gflow.worktree.enabled";
const KEY_EDITOR: &str = "gflow.worktree.editor";
const KEY_PATH: &str = "gflow.worktree.path";

fn scope_label(local: bool) -> &'static str {
    if local { "local (this repo)" } else { "global (all repos)" }
}

/// An editor value that is blank or `none` (any case) means "don't open an editor".
fn editor_disabled(editor: &str) -> bool {
    let editor = editor.trim();
    editor.is_empty() || editor.eq_ignore_ascii_case("none")
}

/// User configuration for the optional worktree flow, read from `gflow.worktree.*`
/// git config keys.
#[derive(Debug)]
pub struct WorktreeConfig {
    pub enabled: bool,
    pub editor: String,
    pub base_path: Option<String>,
}

impl WorktreeConfig {
    /// Load the `gflow.worktree.*` keys. Absent keys fall back to defaults
    /// (disabled, editor `code`, no custom base path). Values are trimmed —
    /// stray whitespace in git config would otherwise break `Command::new`
    /// (e.g. editor `"code "`) or produce oddly named directories.
    pub fn load(git: &dyn Git) -> Result<Self> {
        let enabled = git
            .get_config(KEY_ENABLED)?
            .map(|v| v.trim().eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let editor = git
            .get_config(KEY_EDITOR)?
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "code".to_string());
        let base_path = git
            .get_config(KEY_PATH)?
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        Ok(Self { enabled, editor, base_path })
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

/// Turn the worktree flow on or off.
pub fn set_enabled(git: &dyn Git, enabled: bool, local: bool) -> Result<()> {
    git.set_config(KEY_ENABLED, if enabled { "true" } else { "false" }, !local)?;
    println!(
        "Worktree flow {} — saved to {} git config.",
        if enabled { "enabled" } else { "disabled" },
        scope_label(local),
    );
    Ok(())
}

/// Set the editor command opened for each worktree.
pub fn set_editor(git: &dyn Git, value: &str, local: bool) -> Result<()> {
    git.set_config(KEY_EDITOR, value, !local)?;
    println!("Worktree editor set to '{value}' — saved to {} git config.", scope_label(local));
    Ok(())
}

/// Set the base directory worktree folders are created in.
pub fn set_path(git: &dyn Git, value: &str, local: bool) -> Result<()> {
    git.set_config(KEY_PATH, value, !local)?;
    println!("Worktree base directory set to '{value}' — saved to {} git config.", scope_label(local));
    Ok(())
}

/// Clear a custom base directory, reverting to the default (the repo's parent).
pub fn use_default_path(git: &dyn Git, local: bool) -> Result<()> {
    git.unset_config(KEY_PATH, !local)?;
    println!("Worktree base directory reset to the default (the repo's parent) in {} git config.", scope_label(local));
    Ok(())
}

/// Print the effective worktree configuration.
pub fn show_status(git: &dyn Git) -> Result<()> {
    let cfg = WorktreeConfig::load(git)?;
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
    Ok(())
}

/// Interactive setup: prompts for enable, editor, and location, then saves them.
pub fn wizard(git: &dyn Git, prompter: &dyn Prompter, local: bool) -> Result<()> {
    println!("Configure the worktree flow — writing to {} git config.\n", scope_label(local));

    let enable_items = [
        "Enable — open each new branch in its own worktree + editor",
        "Disable",
    ];
    let enabled = prompter.select("Worktree flow", &enable_items)? == 0;
    set_enabled(git, enabled, local)?;
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
    set_editor(git, &editor_value, local)?;

    // Location: default (repo's parent) or a custom directory.
    let path_items = ["Default — next to the repository", "Custom directory…"];
    if prompter.select("Worktree location", &path_items)? == 0 {
        use_default_path(git, local)?;
    } else {
        let path = prompter.prompt_line("Worktree base directory (e.g. ~/worktrees)")?;
        set_path(git, &path, local)?;
    }

    println!("\nDone. Your next 'gflow start' opens work in a worktree.");
    Ok(())
}

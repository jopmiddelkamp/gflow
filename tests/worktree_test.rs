mod common;

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use common::{MockEditor, MockGit, MockPrompter};
use gflow::worktree::{
    worktree_path, WorktreeConfig, EDITOR_PRESETS,
    set_enabled, set_editor, set_path, use_default_path, show_status, wizard,
};

/// `worktree_path` reads HOME/USERPROFILE, and one test below removes them.
/// Rust runs a test binary's tests as parallel threads in one process, so every
/// test touching those vars takes this lock. (A `serial_test` dev-dependency
/// would do the same thing — the dependency budget says hand-roll it.)
static HOME_ENV: Mutex<()> = Mutex::new(());

// --- worktree_path (pure) ---

#[test]
fn worktree_path_default_base_is_repo_parent_with_prefixed_folder() {
    let root = Path::new("/Users/jop/Projects/beans/beans-gitflow");
    let p = worktree_path(root, "beans-gitflow", None, "feature/login");
    assert_eq!(p, PathBuf::from("/Users/jop/Projects/beans/beans-gitflow-feature-login"));
}

#[test]
fn worktree_path_slashes_become_dashes() {
    let root = Path::new("/repos/beans-gitflow");
    let p = worktree_path(root, "beans-gitflow", None, "release-fix/1.2.0/null-crash");
    assert_eq!(p, PathBuf::from("/repos/beans-gitflow-release-fix-1.2.0-null-crash"));
}

#[test]
fn worktree_path_custom_base_is_used_verbatim() {
    let root = Path::new("/repos/beans-gitflow");
    let p = worktree_path(root, "beans-gitflow", Some("/Users/jop/worktrees"), "feature/login");
    assert_eq!(p, PathBuf::from("/Users/jop/worktrees/beans-gitflow-feature-login"));
}

#[test]
fn worktree_path_expands_leading_tilde_in_custom_base() {
    let _guard = HOME_ENV.lock().unwrap_or_else(|e| e.into_inner());
    // `~` in git config never passes through a shell, so gflow expands it itself.
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap();
    let root = Path::new("/repos/beans-gitflow");
    let p = worktree_path(root, "beans-gitflow", Some("~/worktrees"), "feature/login");
    assert_eq!(p, PathBuf::from(home).join("worktrees/beans-gitflow-feature-login"));
}

// --- WorktreeConfig::load ---

#[test]
fn config_defaults_when_unset() {
    let git = MockGit::new(); // empty config map
    let cfg = WorktreeConfig::load(&git).unwrap();
    assert!(!cfg.enabled);
    assert_eq!(cfg.editor, "code");
    assert_eq!(cfg.base_path, None);
}

#[test]
fn config_reads_all_values() {
    let mut git = MockGit::new();
    git.config.insert("gflow.worktree.enabled".to_string(), "true".to_string());
    git.config.insert("gflow.worktree.editor".to_string(), "cursor".to_string());
    git.config.insert("gflow.worktree.path".to_string(), "/wt".to_string());

    let cfg = WorktreeConfig::load(&git).unwrap();
    assert!(cfg.enabled);
    assert_eq!(cfg.editor, "cursor");
    assert_eq!(cfg.base_path.as_deref(), Some("/wt"));
}

#[test]
fn config_trims_whitespace_from_editor_and_path() {
    // Stray whitespace in git config would otherwise break Command::new("code ")
    // or produce oddly named directories.
    let mut git = MockGit::new();
    git.config.insert("gflow.worktree.editor".to_string(), "code ".to_string());
    git.config.insert("gflow.worktree.path".to_string(), " /wt ".to_string());

    let cfg = WorktreeConfig::load(&git).unwrap();
    assert_eq!(cfg.editor, "code");
    assert_eq!(cfg.base_path.as_deref(), Some("/wt"));
}

#[test]
fn config_whitespace_only_editor_falls_back_to_code() {
    let mut git = MockGit::new();
    git.config.insert("gflow.worktree.editor".to_string(), "   ".to_string());
    let cfg = WorktreeConfig::load(&git).unwrap();
    assert_eq!(cfg.editor, "code");
}

#[test]
fn config_enabled_is_false_when_value_is_not_true() {
    let mut git = MockGit::new();
    git.config.insert("gflow.worktree.enabled".to_string(), "false".to_string());
    let cfg = WorktreeConfig::load(&git).unwrap();
    assert!(!cfg.enabled);
}

#[test]
fn config_empty_editor_falls_back_to_code() {
    let mut git = MockGit::new();
    git.config.insert("gflow.worktree.editor".to_string(), "".to_string());
    let cfg = WorktreeConfig::load(&git).unwrap();
    assert_eq!(cfg.editor, "code");
}

#[test]
fn config_preserves_none_editor() {
    let mut git = MockGit::new();
    git.config.insert("gflow.worktree.editor".to_string(), "none".to_string());
    let cfg = WorktreeConfig::load(&git).unwrap();
    assert_eq!(cfg.editor, "none");
}

// --- `gflow worktree` config setters ---

#[test]
fn set_enabled_writes_global_by_default() {
    let git = MockGit::new();
    set_enabled(&git, true, false).unwrap();
    assert_eq!(git.calls(), vec!["set_config:global:gflow.worktree.enabled:true"]);
}

#[test]
fn set_enabled_false_writes_local_when_requested() {
    let git = MockGit::new();
    set_enabled(&git, false, true).unwrap();
    assert_eq!(git.calls(), vec!["set_config:local:gflow.worktree.enabled:false"]);
}

#[test]
fn set_editor_writes_value_verbatim() {
    let git = MockGit::new();
    set_editor(&git, "cursor", false).unwrap();
    assert_eq!(git.calls(), vec!["set_config:global:gflow.worktree.editor:cursor"]);
}

#[test]
fn set_path_writes_value() {
    let git = MockGit::new();
    set_path(&git, "/Users/jop/worktrees", false).unwrap();
    assert_eq!(git.calls(), vec!["set_config:global:gflow.worktree.path:/Users/jop/worktrees"]);
}

#[test]
fn use_default_path_unsets_the_key() {
    let git = MockGit::new();
    use_default_path(&git, false).unwrap();
    assert_eq!(git.calls(), vec!["unset_config:global:gflow.worktree.path"]);
}

#[test]
fn show_status_reads_the_three_keys() {
    let mut git = MockGit::new();
    git.config.insert("gflow.worktree.enabled".to_string(), "true".to_string());
    show_status(&git).unwrap();
    let calls = git.calls();
    assert!(calls.iter().any(|c| c == "get_config:gflow.worktree.enabled"));
    assert!(calls.iter().any(|c| c == "get_config:gflow.worktree.editor"));
    assert!(calls.iter().any(|c| c == "get_config:gflow.worktree.path"));
}


#[test]
fn editor_presets_map_friendly_names_to_commands() {
    let map = |label: &str| EDITOR_PRESETS.iter().find(|(l, _)| *l == label).map(|(_, c)| *c);
    assert_eq!(map("VS Code"), Some("code"));
    assert_eq!(map("Cursor"), Some("cursor"));
    assert_eq!(map("PyCharm"), Some("pycharm"));
}

#[test]
fn show_status_flags_a_disabled_editor_and_a_custom_path() {
    // The status screen is the only place the effective config is visible, so the
    // two "not the default" shapes have to read correctly: an editor that won't
    // open anything, and a custom base directory.
    let mut git = MockGit::new();
    git.config.insert("gflow.worktree.enabled".to_string(), "true".to_string());
    git.config.insert("gflow.worktree.editor".to_string(), "none".to_string());
    git.config.insert("gflow.worktree.path".to_string(), "~/worktrees".to_string());

    show_status(&git).unwrap();

    assert_eq!(git.calls(), vec![
        "get_config:gflow.worktree.enabled",
        "get_config:gflow.worktree.editor",
        "get_config:gflow.worktree.path",
    ], "status is read-only — it must never write config");
}

#[test]
fn show_status_on_a_disabled_flow_reads_the_same_three_keys() {
    let git = MockGit::new(); // nothing configured: disabled, editor `code`, default path

    show_status(&git).unwrap();

    assert!(!git.calls().iter().any(|c| c.starts_with("set_config") || c.starts_with("unset_config")),
        "calls: {:?}", git.calls());
}

#[test]
fn worktree_path_without_a_home_directory_keeps_the_tilde_literal() {
    // No HOME and no USERPROFILE (a bare CI container): expansion is impossible,
    // so the value is used verbatim rather than silently rooted somewhere else.
    let _guard = HOME_ENV.lock().unwrap_or_else(|e| e.into_inner());
    let saved_home = std::env::var_os("HOME");
    let saved_profile = std::env::var_os("USERPROFILE");
    std::env::remove_var("HOME");
    std::env::remove_var("USERPROFILE");

    let path = worktree_path(Path::new("/repos/app"), "app", Some("~/wt"), "feature/x");

    if let Some(h) = saved_home { std::env::set_var("HOME", h); }
    if let Some(p) = saved_profile { std::env::set_var("USERPROFILE", p); }

    assert_eq!(path, PathBuf::from("~/wt/app-feature-x"));
}

#[test]
fn an_unusable_worktree_base_directory_is_a_hard_error() {
    // Creating the worktree is the whole point of the flow — unlike the editor
    // open, a base directory gflow cannot create is fatal and names the path.
    use gflow::worktree::open_worktree;
    let blocker = std::env::temp_dir().join("gflow-wt-blocker-file");
    std::fs::write(&blocker, b"not a directory").unwrap();
    let mut git = MockGit::new();
    git.repo_root = PathBuf::from("/repos/app");
    let config = WorktreeConfig {
        enabled: true,
        editor: "code".to_string(),
        // The parent of the computed worktree path is a regular file.
        base_path: Some(blocker.join("nested").to_string_lossy().to_string()),
    };

    let env = gflow::worktree::WorktreeEnv { config: &config, editor: &MockEditor::new(), setup: &common::MockWorktreeSetup::new(), commands: None };
    let err = open_worktree(&git, &gflow::worktree::WorktreeContext { env: &env, prompter: &MockPrompter::new() }, "feature/x").unwrap_err();

    std::fs::remove_file(&blocker).ok();
    assert!(err.contains("failed to create worktree base directory"), "got: {err}");
    assert!(!git.calls().iter().any(|c| c.starts_with("add_worktree")),
        "no worktree may be added after the directory failure; calls: {:?}", git.calls());
}

// --- `gflow worktree` wizard ---
//
// The wizard is the setter functions plus a script of prompts; what matters is
// which config keys it writes, in which scope, and where it stops early.

#[test]
fn wizard_disable_short_circuits_before_the_editor_and_path_prompts() {
    let git = MockGit::new();
    let prompter = MockPrompter::scripted(&[1]); // "Disable"

    wizard(&git, &prompter, false).unwrap();

    assert_eq!(git.calls(), vec!["set_config:global:gflow.worktree.enabled:false"],
        "a disabled flow needs no editor or location");
    assert_eq!(prompter.calls().len(), 1, "prompts: {:?}", prompter.calls());
}

#[test]
fn wizard_enable_with_a_preset_editor_and_the_default_location() {
    let git = MockGit::new();
    // enable, first editor preset, default location
    let prompter = MockPrompter::scripted(&[0, 0, 0]);

    wizard(&git, &prompter, false).unwrap();

    assert_eq!(git.calls(), vec![
        "set_config:global:gflow.worktree.enabled:true".to_string(),
        format!("set_config:global:gflow.worktree.editor:{}", EDITOR_PRESETS[0].1),
        // "use default" unsets the key rather than writing a value.
        "unset_config:global:gflow.worktree.path".to_string(),
    ]);
}

#[test]
fn wizard_none_editor_entry_sits_after_the_presets() {
    let git = MockGit::new();
    let none_idx = EDITOR_PRESETS.len();
    let prompter = MockPrompter::scripted(&[0, none_idx, 0]);

    wizard(&git, &prompter, false).unwrap();

    assert!(git.calls().contains(&"set_config:global:gflow.worktree.editor:none".to_string()),
        "calls: {:?}", git.calls());
}

#[test]
fn wizard_custom_editor_and_custom_path_are_read_as_free_text() {
    let git = MockGit::new();
    let custom_idx = EDITOR_PRESETS.len() + 1;
    let prompter = MockPrompter::scripted(&[0, custom_idx, 1])
        .with_lines(&["my-editor --wait", "~/worktrees"]);

    wizard(&git, &prompter, true).unwrap();

    assert_eq!(git.calls(), vec![
        // --local flips every write to repo scope.
        "set_config:local:gflow.worktree.enabled:true",
        "set_config:local:gflow.worktree.editor:my-editor --wait",
        "set_config:local:gflow.worktree.path:~/worktrees",
    ]);
    assert_eq!(prompter.calls(), vec![
        "select:Worktree flow:[Enable — open each new branch in its own worktree + editor, Disable]",
        "select:Editor:[VS Code  (code), Cursor  (cursor), Windsurf  (windsurf), Zed  (zed), IntelliJ IDEA  (idea), PyCharm  (pycharm), WebStorm  (webstorm), None — create the worktree but don't open an editor, Custom command…]",
        // Both free-text reads use prompt_line: prompt_name would mangle a
        // command's spaces and a path's tilde/slashes into a branch slug.
        "prompt_line:Editor command (e.g. code, cursor)",
        "select:Worktree location:[Default — next to the repository, Custom directory…]",
        "prompt_line:Worktree base directory (e.g. ~/worktrees)",
    ]);
}

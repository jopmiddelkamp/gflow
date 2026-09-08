mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use common::{MockEditor, MockGit, MockPrompter};
use gflow::cli::WorktreeAction;
use gflow::repo_config::Settings;
use gflow::worktree::{
    config_target, ConfigScope,
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

// --- WorktreeConfig::from_settings ---

fn settings(text: &str) -> Settings {
    gflow::repo_config::parse(text).unwrap()
}

fn target(prefix: &str) -> (common::TempDir, PathBuf) {
    let dir = common::tmp_dir(prefix);
    let path = dir.join("config");
    (dir, path)
}

#[test]
fn config_defaults_when_unset() {
    let cfg = WorktreeConfig::from_settings(&settings(""));
    assert!(!cfg.enabled);
    assert_eq!(cfg.editor, "code");
    assert_eq!(cfg.base_path, None);
}

#[test]
fn config_reads_all_values() {
    let cfg = WorktreeConfig::from_settings(&settings("worktree=true\neditor=cursor\npath=/wt\n"));
    assert!(cfg.enabled);
    assert_eq!(cfg.editor, "cursor");
    assert_eq!(cfg.base_path.as_deref(), Some("/wt"));
}

#[test]
fn config_trims_whitespace_from_editor_and_path() {
    // Stray whitespace would otherwise break Command::new("code ") or produce
    // oddly named directories. Hand-built Settings, so this pins the conversion
    // rather than the parser's own trimming.
    let raw = Settings { editor: Some("code ".into()), path: Some(" /wt ".into()), ..Settings::default() };
    let cfg = WorktreeConfig::from_settings(&raw);
    assert_eq!(cfg.editor, "code");
    assert_eq!(cfg.base_path.as_deref(), Some("/wt"));
}

#[test]
fn config_whitespace_only_editor_falls_back_to_code() {
    let cfg = WorktreeConfig::from_settings(&settings("editor=   \n"));
    assert_eq!(cfg.editor, "code");
}

#[test]
fn config_enabled_is_false_when_the_key_says_false() {
    let cfg = WorktreeConfig::from_settings(&settings("worktree=false\n"));
    assert!(!cfg.enabled);
}

#[test]
fn config_preserves_none_editor() {
    let cfg = WorktreeConfig::from_settings(&settings("editor=none\n"));
    assert_eq!(cfg.editor, "none");
}

// --- `gflow worktree` config setters ---
//
// The setters write config files now, so what matters is the bytes that land
// on disk, not a git config call sequence.

#[test]
fn set_enabled_writes_the_key_to_the_target_file() {
    let (_dir, path) = target("gflow-wt-enable");
    set_enabled(&path, true, ConfigScope::Global).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "worktree=true\n");
}

#[test]
fn set_enabled_false_writes_false() {
    let (_dir, path) = target("gflow-wt-disable");
    set_enabled(&path, false, ConfigScope::Local).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "worktree=false\n");
}

#[test]
fn set_editor_writes_value_verbatim() {
    let (_dir, path) = target("gflow-wt-editor");
    set_editor(&path, "cursor", ConfigScope::Global).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "editor=cursor\n");
}

#[test]
fn set_path_writes_value() {
    let (_dir, path) = target("gflow-wt-path");
    set_path(&path, "/Users/jop/worktrees", ConfigScope::Global).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "path=/Users/jop/worktrees\n");
}

#[test]
fn use_default_path_removes_the_key() {
    let (_dir, path) = target("gflow-wt-default");
    fs::write(&path, "worktree=true\npath=/wt\n").unwrap();
    use_default_path(&path, ConfigScope::Global).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "worktree=true\n");
}

#[test]
fn a_setter_keeps_every_other_key_in_the_file() {
    let (_dir, path) = target("gflow-wt-keep");
    fs::write(&path, "mode=protected\nworktree=false\n").unwrap();
    set_enabled(&path, true, ConfigScope::Global).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "mode=protected\nworktree=true\n");
}

#[test]
fn show_status_renders_both_shapes_without_writing_anything() {
    show_status(&WorktreeConfig::from_settings(&settings("worktree=true\neditor=none\npath=~/worktrees\n")));
    show_status(&WorktreeConfig::from_settings(&settings("")));
}

#[test]
fn editor_presets_map_friendly_names_to_commands() {
    let map = |label: &str| EDITOR_PRESETS.iter().find(|(l, _)| *l == label).map(|(_, c)| *c);
    assert_eq!(map("VS Code"), Some("code"));
    assert_eq!(map("Cursor"), Some("cursor"));
    assert_eq!(map("PyCharm"), Some("pycharm"));
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
    let (_dir, path) = target("gflow-wt-wiz-off");
    let prompter = MockPrompter::scripted(&[1]); // "Disable"

    wizard(&path, &prompter, ConfigScope::Global).unwrap();

    assert_eq!(fs::read_to_string(&path).unwrap(), "worktree=false\n",
        "a disabled flow needs no editor or location");
    assert_eq!(prompter.calls().len(), 1, "prompts: {:?}", prompter.calls());
}

#[test]
fn wizard_enable_with_a_preset_editor_and_the_default_location() {
    let (_dir, path) = target("gflow-wt-wiz-on");
    // enable, first editor preset, default location
    let prompter = MockPrompter::scripted(&[0, 0, 0]);

    wizard(&path, &prompter, ConfigScope::Global).unwrap();

    // "use default" removes the path key rather than writing a value.
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        format!("worktree=true\neditor={}\n", EDITOR_PRESETS[0].1)
    );
}

#[test]
fn wizard_none_editor_entry_sits_after_the_presets() {
    let (_dir, path) = target("gflow-wt-wiz-none");
    let none_idx = EDITOR_PRESETS.len();
    let prompter = MockPrompter::scripted(&[0, none_idx, 0]);

    wizard(&path, &prompter, ConfigScope::Global).unwrap();

    assert!(fs::read_to_string(&path).unwrap().contains("editor=none"));
}

#[test]
fn wizard_custom_editor_and_custom_path_are_read_as_free_text() {
    let (_dir, path) = target("gflow-wt-wiz-custom");
    let custom_idx = EDITOR_PRESETS.len() + 1;
    let prompter = MockPrompter::scripted(&[0, custom_idx, 1])
        .with_lines(&["my-editor --wait", "~/worktrees"]);

    wizard(&path, &prompter, ConfigScope::Local).unwrap();

    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "worktree=true\neditor=my-editor --wait\npath=~/worktrees\n"
    );
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

// --- which config file `gflow worktree` writes ---

#[test]
fn scope_defaults_to_the_all_repos_file() {
    assert_eq!(ConfigScope::from_flags(false, false), ConfigScope::Global);
}

#[test]
fn repo_flag_selects_the_committed_file_and_local_flag_the_private_one() {
    assert_eq!(ConfigScope::from_flags(true, false), ConfigScope::Repo);
    assert_eq!(ConfigScope::from_flags(false, true), ConfigScope::Local);
}

#[test]
fn config_target_global_is_the_all_repos_file() {
    let repo = PathBuf::from("/repos/app");
    let target =
        config_target(Some(&repo), Some(Path::new("/home/jop")), ConfigScope::Global).unwrap();
    assert_eq!(target, PathBuf::from("/home/jop/.gflow/config"));
}

#[test]
fn config_target_repo_is_the_repositorys_committed_config() {
    let repo = PathBuf::from("/repos/app");
    let target =
        config_target(Some(&repo), Some(Path::new("/home/jop")), ConfigScope::Repo).unwrap();
    assert_eq!(target, PathBuf::from("/repos/app/.gflow/config"));
}

#[test]
fn config_target_local_is_the_private_per_repo_override() {
    let repo = PathBuf::from("/repos/app");
    let target =
        config_target(Some(&repo), Some(Path::new("/home/jop")), ConfigScope::Local).unwrap();
    assert_eq!(target, PathBuf::from("/repos/app/.gflow/config.local"));
}

#[test]
fn a_repo_scope_outside_a_repository_names_the_problem() {
    for scope in [ConfigScope::Repo, ConfigScope::Local] {
        let err = config_target(None, Some(Path::new("/home/jop")), scope).unwrap_err();
        assert!(err.contains("git repository"), "got: {err}");
    }
}

#[test]
fn config_target_without_a_home_directory_points_at_the_repo_remedies() {
    let repo = PathBuf::from("/repos/app");
    let err = config_target(Some(&repo), None, ConfigScope::Global).unwrap_err();
    assert!(err.contains("--repo"), "got: {err}");
}

// --- `gflow worktree` dispatch ---

#[test]
fn run_config_enable_writes_the_all_repos_file() {
    let home = common::tmp_dir("gflow-run-home");
    let repo = common::tmp_dir("gflow-run-repo");

    gflow::worktree::run_config(
        &MockPrompter::aborting(),
        Some(&repo),
        Some(&home),
        Some(WorktreeAction::Enable),
        ConfigScope::Global,
    )
    .unwrap();

    assert_eq!(fs::read_to_string(home.join(".gflow").join("config")).unwrap(), "worktree=true\n");
    assert!(!repo.join(".gflow").exists(), "a global write must not touch the repo");
}

#[test]
fn run_config_repo_writes_the_committed_config() {
    let home = common::tmp_dir("gflow-run-home");
    let repo = common::tmp_dir("gflow-run-repo");

    gflow::worktree::run_config(
        &MockPrompter::aborting(),
        Some(&repo),
        Some(&home),
        Some(WorktreeAction::Editor { value: "zed".into() }),
        ConfigScope::Repo,
    )
    .unwrap();

    assert_eq!(fs::read_to_string(repo.join(".gflow").join("config")).unwrap(), "editor=zed\n");
    assert!(!home.join(".gflow").exists(), "a repo write must not touch the global file");
}

#[test]
fn run_config_status_reads_the_layers_and_writes_nothing() {
    let home = common::tmp_dir("gflow-run-home");
    let repo = common::tmp_dir("gflow-run-repo");
    fs::create_dir_all(home.join(".gflow")).unwrap();
    fs::write(home.join(".gflow").join("config"), "worktree=true\neditor=none\n").unwrap();

    gflow::worktree::run_config(
        &MockPrompter::aborting(),
        Some(&repo),
        Some(&home),
        Some(WorktreeAction::Status),
        ConfigScope::Global,
    )
    .unwrap();

    assert_eq!(
        fs::read_to_string(home.join(".gflow").join("config")).unwrap(),
        "worktree=true\neditor=none\n",
        "status is read-only"
    );
    assert!(!repo.join(".gflow").exists());
}

#[test]
fn run_config_path_and_disable_reach_their_setters() {
    let home = common::tmp_dir("gflow-run-home");
    let p = &MockPrompter::aborting();
    gflow::worktree::run_config(p, None, Some(&home), Some(WorktreeAction::Disable), ConfigScope::Global).unwrap();
    gflow::worktree::run_config(
        p,
        None,
        Some(&home),
        Some(WorktreeAction::Path { value: "~/wt".into() }),
        ConfigScope::Global,
    )
    .unwrap();

    assert_eq!(
        fs::read_to_string(home.join(".gflow").join("config")).unwrap(),
        "worktree=false\npath=~/wt\n"
    );
}

#[test]
fn run_config_with_no_action_runs_the_wizard() {
    let home = common::tmp_dir("gflow-run-home");
    let prompter = MockPrompter::scripted(&[1]); // "Disable"

    gflow::worktree::run_config(&prompter, None, Some(&home), None, ConfigScope::Global).unwrap();

    assert_eq!(fs::read_to_string(home.join(".gflow").join("config")).unwrap(), "worktree=false\n");
}

#[test]
fn run_config_local_writes_the_private_override_and_ignores_it() {
    let repo = common::tmp_dir("gflow-run-repo");

    gflow::worktree::run_config(
        &MockPrompter::aborting(),
        Some(&repo),
        None,
        Some(WorktreeAction::Enable),
        ConfigScope::Local,
    )
    .unwrap();

    let dir = repo.join(".gflow");
    assert_eq!(fs::read_to_string(dir.join("config.local")).unwrap(), "worktree=true\n");
    assert!(fs::read_to_string(dir.join(".gitignore")).unwrap().contains("config.local"));
    assert!(!dir.join("config").exists(), "the committed file is untouched");
}

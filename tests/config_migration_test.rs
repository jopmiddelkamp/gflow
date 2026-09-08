//! Upgrading from 4.0.x must be lossless and silent: the `gflow.worktree.*`
//! git config keys move into the layered config files, in the scope they were
//! set in, and the git keys are unset so the migration never runs twice.

mod common;

use std::fs;

use common::{tmp_dir, MockGit};
use gflow::repo_config;

fn read(path: &std::path::Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

#[test]
fn a_global_worktree_setting_moves_into_the_global_config_file() {
    let home = tmp_dir("gflow-migrate-home");
    let mut git = MockGit::new();
    git.config_global.insert("gflow.worktree.enabled".into(), "true".into());
    git.config_global.insert("gflow.worktree.editor".into(), "cursor".into());
    git.config_global.insert("gflow.worktree.path".into(), "~/wt".into());

    repo_config::migrate_git_config(&git, Some(&home), None).unwrap();

    let contents = read(&home.join(".gflow").join("config"));
    let (settings, _) = repo_config::parse(&contents).map(|s| (s, ())).unwrap();
    assert_eq!(settings.worktree, Some(true));
    assert_eq!(settings.editor.as_deref(), Some("cursor"));
    assert_eq!(settings.path.as_deref(), Some("~/wt"));
}

#[test]
fn migrated_global_keys_are_unset_so_the_migration_never_runs_twice() {
    let home = tmp_dir("gflow-migrate-home");
    let mut git = MockGit::new();
    git.config_global.insert("gflow.worktree.enabled".into(), "true".into());

    repo_config::migrate_git_config(&git, Some(&home), None).unwrap();

    assert!(
        git.calls().contains(&"unset_config:global:gflow.worktree.enabled".to_string()),
        "got: {:?}",
        git.calls()
    );
}

#[test]
fn a_local_worktree_setting_moves_into_the_private_override() {
    let home = tmp_dir("gflow-migrate-home");
    let repo = tmp_dir("gflow-migrate-repo");
    let mut git = MockGit::new();
    git.config.insert("gflow.worktree.enabled".into(), "true".into());

    repo_config::migrate_git_config(&git, Some(&home), Some(&repo)).unwrap();

    let contents = read(&repo.join(".gflow").join("config.local"));
    assert!(contents.contains("worktree=true"), "got: {contents:?}");
    assert!(
        !repo.join(".gflow").join("config").exists(),
        "local git config was never committed; migrating it must not publish it"
    );
    assert!(
        read(&repo.join(".gflow").join(".gitignore")).contains("config.local"),
        "the private layer must be ignored by git"
    );
    assert_eq!(
        read(&home.join(".gflow").join("config")),
        "",
        "a local setting must not leak into the global file"
    );
    assert!(
        git.calls().contains(&"unset_config:local:gflow.worktree.enabled".to_string()),
        "got: {:?}",
        git.calls()
    );
}

#[test]
fn migration_never_overwrites_a_value_the_file_already_states() {
    let home = tmp_dir("gflow-migrate-home");
    fs::create_dir_all(home.join(".gflow")).unwrap();
    fs::write(home.join(".gflow").join("config"), "worktree=false\n").unwrap();
    let mut git = MockGit::new();
    git.config_global.insert("gflow.worktree.enabled".into(), "true".into());

    repo_config::migrate_git_config(&git, Some(&home), None).unwrap();

    let contents = read(&home.join(".gflow").join("config"));
    assert!(contents.contains("worktree=false"), "the file wins: {contents:?}");
    assert!(
        git.calls().contains(&"unset_config:global:gflow.worktree.enabled".to_string()),
        "the stale git key is still cleaned up"
    );
}

#[test]
fn nothing_to_migrate_writes_no_file() {
    let home = tmp_dir("gflow-migrate-home");
    let repo = tmp_dir("gflow-migrate-repo");
    let git = MockGit::new();

    repo_config::migrate_git_config(&git, Some(&home), Some(&repo)).unwrap();

    assert!(!home.join(".gflow").exists(), "no settings, no files");
    assert!(!repo.join(".gflow").exists(), "no settings, no files");
}

#[test]
fn a_second_run_is_a_no_op() {
    let home = tmp_dir("gflow-migrate-home");
    let mut git = MockGit::new();
    git.config_global.insert("gflow.worktree.editor".into(), "zed".into());
    repo_config::migrate_git_config(&git, Some(&home), None).unwrap();
    let after_first = read(&home.join(".gflow").join("config"));

    let clean = MockGit::new();
    repo_config::migrate_git_config(&clean, Some(&home), None).unwrap();

    assert_eq!(read(&home.join(".gflow").join("config")), after_first);
}

#[test]
fn detection_caches_are_not_settings_and_stay_in_git_config() {
    let home = tmp_dir("gflow-migrate-home");
    let repo = tmp_dir("gflow-migrate-repo");
    let mut git = MockGit::new();
    git.config.insert("gflow.branch.main".into(), "master".into());
    git.config.insert("gflow.hosting.provider".into(), "github".into());

    repo_config::migrate_git_config(&git, Some(&home), Some(&repo)).unwrap();

    let unset: Vec<_> = git.calls().into_iter().filter(|c| c.starts_with("unset_config")).collect();
    assert!(unset.is_empty(), "caches must be left alone, got: {unset:?}");
}

#[test]
fn migration_appends_cleanly_to_a_file_with_no_trailing_newline() {
    let home = tmp_dir("gflow-migrate-home");
    fs::create_dir_all(home.join(".gflow")).unwrap();
    fs::write(home.join(".gflow").join("config"), "mode=protected").unwrap();
    let mut git = MockGit::new();
    git.config_global.insert("gflow.worktree.enabled".into(), "TRUE".into());

    repo_config::migrate_git_config(&git, Some(&home), None).unwrap();

    assert_eq!(
        read(&home.join(".gflow").join("config")),
        "mode=protected\nworktree=true\n",
        "git config accepted any casing; the file format does not"
    );
}

#[test]
fn a_machine_with_no_home_still_migrates_the_repositorys_local_settings() {
    // A bare CI container states neither HOME nor USERPROFILE. The global layer
    // is simply unavailable; the repo's own settings must still move.
    let repo = tmp_dir("gflow-migrate-repo");
    let mut git = MockGit::new();
    git.config.insert("gflow.worktree.editor".into(), "zed".into());

    repo_config::migrate_git_config(&git, None, Some(&repo)).unwrap();

    assert_eq!(read(&repo.join(".gflow").join("config.local")), "editor=zed\n");
    assert!(
        git.calls().contains(&"unset_config:local:gflow.worktree.editor".to_string()),
        "got: {:?}",
        git.calls()
    );
}

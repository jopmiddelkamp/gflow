//! Public behavior of the layered config files.
//!
//! Upgrading from 4.0.x must be lossless and silent: the `gflow.worktree.*`
//! git config keys move into the layered config files, in the scope they were
//! set in, and the git keys are unset so the migration never runs twice.

mod common;

use std::fs;

use common::{tmp_dir, MockCommandRunner, MockGit};
use gflow::git::GitCli;
use gflow::repo_config::{self, RepoConfig};

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
    for key in ["enabled", "editor", "path"] {
        assert!(git.calls().contains(&format!("unset_config:global:gflow.worktree.{key}")));
    }
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
fn the_migration_reads_each_scope_with_one_git_call() {
    // It runs before every menu; three reads per scope cost a spawn each.
    let home = tmp_dir("gflow-migrate-home");
    let repo = tmp_dir("gflow-migrate-repo");
    let git = MockGit::new();

    repo_config::migrate_git_config(&git, Some(&home), Some(&repo)).unwrap();

    assert_eq!(git.calls(), vec![
        "config_section_at:global:gflow.worktree",
        "config_section_at:local:gflow.worktree",
    ]);
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

#[test]
fn config_migration_preserves_git_settings_when_the_destination_cannot_be_created() {
    let home = tmp_dir("gflow-migration-blocked");
    fs::write(home.join(".gflow"), "keep this file").unwrap();
    let mut git = MockGit::new();
    git.config_global
        .insert("gflow.worktree.editor".into(), "zed".into());

    let err = repo_config::migrate_git_config(&git, Some(&home), None).unwrap_err();

    assert!(err.contains("Failed to create"), "got: {err}");
    assert!(
        !git.calls()
            .iter()
            .any(|call| call.starts_with("unset_config:")),
        "the only saved copy must survive a failed migration: {:?}",
        git.calls()
    );
    assert_eq!(
        fs::read_to_string(home.join(".gflow")).unwrap(),
        "keep this file"
    );
}

#[test]
fn config_read_errors_identify_the_unreadable_file() {
    let root = tmp_dir("gflow-config-unreadable");
    let path = repo_config::config_path(&root);
    fs::create_dir_all(&path).unwrap();

    for result in [
        repo_config::load_layers(Some(&root), None).map(|_| ()),
        repo_config::load_layers(None, Some(&root)).map(|_| ()),
        repo_config::set_key(&path, "editor", Some("zed")),
        repo_config::migrate_git_config(&MockGit::new(), Some(&root), None),
    ] {
        let err = result.unwrap_err();
        assert!(err.contains("Failed to read"), "got: {err}");
        assert!(err.contains(&path.display().to_string()), "got: {err}");
    }
}

#[test]
fn unreadable_private_config_is_an_error_instead_of_an_empty_layer() {
    let root = tmp_dir("gflow-private-config-unreadable");
    let path = repo_config::local_config_path(&root);
    fs::create_dir_all(&path).unwrap();

    let err = repo_config::load_layers(None, Some(&root)).unwrap_err();

    assert!(err.contains("Failed to read"), "got: {err}");
    assert!(err.contains("config.local"), "got: {err}");
}

#[test]
fn invalid_config_layers_stop_loading_and_migration() {
    for name in ["config", "config.local"] {
        let root = tmp_dir("gflow-invalid-layer");
        fs::create_dir_all(root.join(".gflow")).unwrap();
        fs::write(root.join(".gflow").join(name), "worktree=maybe\n").unwrap();

        let err = repo_config::load_layers(None, Some(&root)).unwrap_err();
        assert!(err.contains("Invalid worktree"), "got: {err}");
    }
    let home = tmp_dir("gflow-invalid-migration");
    fs::create_dir_all(home.join(".gflow")).unwrap();
    fs::write(repo_config::global_config_path(&home), "not a setting").unwrap();
    let git = MockGit::new();

    assert!(repo_config::migrate_git_config(&git, Some(&home), None).is_err());
    assert!(
        git.calls().is_empty(),
        "invalid config must stop before changing Git"
    );
}

#[test]
fn blocked_config_directories_report_the_path_and_preserve_the_blocker() {
    let root = tmp_dir("gflow-config-blocker");
    let blocker = root.join(".gflow");
    fs::write(&blocker, "keep me").unwrap();

    for result in [
        repo_config::write(&root, &RepoConfig::default()),
        repo_config::set_key(&blocker.join("config"), "editor", Some("zed")),
    ] {
        let err = result.unwrap_err();
        assert!(err.contains("Failed to create"), "got: {err}");
        assert!(err.contains(&blocker.display().to_string()), "got: {err}");
    }
    assert_eq!(fs::read_to_string(&blocker).unwrap(), "keep me");
}

#[test]
fn config_write_errors_do_not_replace_a_directory() {
    let root = tmp_dir("gflow-config-write-error");
    let path = repo_config::config_path(&root);
    fs::create_dir_all(&path).unwrap();

    let err = repo_config::write(&root, &RepoConfig::default()).unwrap_err();

    assert!(err.contains("Failed to write"), "got: {err}");
    assert!(path.is_dir());
}

#[cfg(unix)]
#[test]
fn broken_destination_links_report_write_errors_without_removing_old_settings() {
    for name in ["config", "config.local", ".gitignore"] {
        let root = tmp_dir("gflow-config-write-link");
        let directory = root.join(".gflow");
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join(name);
        std::os::unix::fs::symlink(root.join("missing/target"), &path).unwrap();
        let mut git = MockGit::new();
        git.config_global
            .insert("gflow.worktree.editor".into(), "zed".into());
        git.config
            .insert("gflow.worktree.editor".into(), "zed".into());

        let result = match name {
            "config" => repo_config::migrate_git_config(&git, Some(&root), None),
            "config.local" => repo_config::migrate_git_config(&git, None, Some(&root)),
            _ => repo_config::migrate_git_config(&git, None, Some(&root)),
        };

        let err = result.unwrap_err();
        assert!(err.contains("Failed to write"), "got: {err}");
        assert!(
            !git.calls()
                .iter()
                .any(|call| call.starts_with("unset_config:")),
            "migration must retain the old copy: {:?}",
            git.calls()
        );
        assert!(path.symlink_metadata().unwrap().file_type().is_symlink());
        if name == "config" {
            assert!(repo_config::set_key(&path, "editor", Some("zed"))
                .unwrap_err()
                .contains("Failed to write"));
        }
    }
}

#[test]
fn migration_discards_blank_personal_values_instead_of_overriding_defaults() {
    let home = tmp_dir("gflow-migration-blank");
    let mut git = MockGit::new();
    git.config_global
        .insert("gflow.worktree.editor".into(), " \t".into());
    git.config_global
        .insert("gflow.worktree.path".into(), "".into());

    repo_config::migrate_git_config(&git, Some(&home), None).unwrap();

    assert!(!repo_config::global_config_path(&home).exists());
    assert!(git
        .calls()
        .contains(&"unset_config:global:gflow.worktree.editor".into()));
    assert!(git
        .calls()
        .contains(&"unset_config:global:gflow.worktree.path".into()));
}

#[test]
fn migration_stops_when_git_config_cannot_be_read() {
    let home = tmp_dir("gflow-migration-read-failure");
    let runner = MockCommandRunner::scripted(&[(2, "", "cannot read config")]);
    let git = GitCli::new(&runner);

    let err = repo_config::migrate_git_config(&git, Some(&home), None).unwrap_err();

    assert!(err.contains("cannot read config"), "got: {err}");
    assert!(!repo_config::global_config_path(&home).exists());
    assert_eq!(
        runner.calls(),
        vec![r"git config --global --get-regexp ^gflow\.worktree\."]
    );
}

#[test]
fn migration_keeps_the_new_copy_when_removing_an_old_git_key_fails() {
    let home = tmp_dir("gflow-migration-unset-failure");
    let runner = MockCommandRunner::scripted(&[
        (0, "gflow.worktree.enabled true", ""),
        (2, "", "cannot lock config"),
    ]);
    let git = GitCli::new(&runner);

    let err = repo_config::migrate_git_config(&git, Some(&home), None).unwrap_err();

    assert!(err.contains("cannot lock config"), "got: {err}");
    assert_eq!(
        fs::read_to_string(repo_config::global_config_path(&home)).unwrap(),
        "worktree=true\n"
    );
}

mod common;

use std::fs;

use common::{tmp_dir, MockCommandRunner, MockGit};
use gflow::git::GitCli;
use gflow::repo_config::{self, RepoConfig};
use gflow::state::{FinishKind, FinishState, LEGACY_STATE_FILE_NAME};

fn release() -> FinishState {
    FinishState {
        kind: FinishKind::Release,
        major: 1,
        minor: 2,
        patch: 0,
        started_at: "1234".into(),
        stash_message: None,
    }
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
        vec!["git config --global --get gflow.worktree.enabled"]
    );
}

#[test]
fn migration_keeps_the_new_copy_when_removing_an_old_git_key_fails() {
    let home = tmp_dir("gflow-migration-unset-failure");
    let runner = MockCommandRunner::scripted(&[
        (0, "true", ""),
        (1, "", ""),
        (1, "", ""),
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

#[test]
fn state_save_reports_an_uncreatable_state_directory() {
    let root = tmp_dir("gflow-state-directory-error");
    fs::write(FinishState::dir(&root), "keep me").unwrap();

    let err = release().save(&root).unwrap_err();

    assert!(err.contains("Failed to create"), "got: {err}");
    assert_eq!(
        fs::read_to_string(FinishState::dir(&root)).unwrap(),
        "keep me"
    );
}

#[test]
fn state_operations_report_file_errors_without_deleting_a_directory() {
    let root = tmp_dir("gflow-state-file-error");
    let path = FinishState::path(&root, FinishKind::Release, 1, 2, 0);
    fs::create_dir_all(&path).unwrap();

    for (operation, result) in [
        (
            "read",
            FinishState::load(&root, FinishKind::Release, 1, 2, 0).map(|_| ()),
        ),
        ("write", release().save(&root)),
        (
            "remove",
            FinishState::clear(&root, FinishKind::Release, 1, 2, 0),
        ),
    ] {
        let err = result.unwrap_err();
        assert!(
            err.contains(&format!("Failed to {operation}")),
            "got: {err}"
        );
        assert!(err.contains(&path.display().to_string()), "got: {err}");
    }
    assert!(path.is_dir());
}

#[cfg(unix)]
#[test]
fn legacy_migration_reports_a_delete_error_after_copying_the_state() {
    use std::os::unix::fs::PermissionsExt;

    let root = tmp_dir("gflow-legacy-delete-error");
    let contents = "version=1\nkind=release\nmajor=1\nminor=2\npatch=0\nstarted_at=1234\n";
    fs::write(root.join(LEGACY_STATE_FILE_NAME), contents).unwrap();
    fs::create_dir(FinishState::dir(&root)).unwrap();
    let permissions = fs::metadata(&*root).unwrap().permissions();
    fs::set_permissions(&*root, fs::Permissions::from_mode(0o555)).unwrap();
    let probe = root.join("write-probe");
    if fs::write(&probe, "").is_ok() {
        fs::set_permissions(&*root, permissions).unwrap();
        eprintln!(
            "Skipping deletion denial: this account or filesystem bypasses directory permissions."
        );
        return;
    }

    let result = FinishState::migrate_legacy(&root);

    fs::set_permissions(&*root, permissions).unwrap();
    let err = result.unwrap_err();
    assert!(err.contains("Failed to remove"), "got: {err}");
    assert!(root.join(LEGACY_STATE_FILE_NAME).exists());
    assert_eq!(
        FinishState::load(&root, FinishKind::Release, 1, 2, 0).unwrap(),
        Some(release())
    );
}

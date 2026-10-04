mod common;

use std::fs;

use common::tmp_dir;
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

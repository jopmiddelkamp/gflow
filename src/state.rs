use std::fs;
use std::path::{Path, PathBuf};

use crate::version::SemVer;

/// Folder under `.git/` holding one state file per in-progress finish.
pub const STATE_DIR_NAME: &str = "gflow-finish";
/// Pre-2.4 single global state file, migrated on startup if found.
pub const LEGACY_STATE_FILE_NAME: &str = "gflow-finish.state";
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinishKind {
    Release,
    Hotfix,
}

impl FinishKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Release => "release",
            Self::Hotfix => "hotfix",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "release" => Some(Self::Release),
            "hotfix" => Some(Self::Hotfix),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinishState {
    pub kind: FinishKind,
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub started_at: String,
    /// Indices shift, so the stash is found by this message before popping.
    pub stash_message: Option<String>,
}

impl FinishState {
    pub fn source_branch(&self) -> String {
        let version = SemVer::new(self.major, self.minor, self.patch);
        match self.kind {
            FinishKind::Release => version.release_branch(),
            FinishKind::Hotfix => version.hotfix_branch(),
        }
    }

    /// Directory holding the per-branch state files.
    pub fn dir(git_dir: &Path) -> PathBuf {
        git_dir.join(STATE_DIR_NAME)
    }

    /// State file name for a given source branch, e.g. `hotfix-2.5.2.state`.
    pub fn file_name(kind: FinishKind, major: u32, minor: u32, patch: u32) -> String {
        format!("{}-{}.{}.{}.state", kind.as_str(), major, minor, patch)
    }

    /// Full path to the state file for a specific source branch.
    pub fn path(git_dir: &Path, kind: FinishKind, major: u32, minor: u32, patch: u32) -> PathBuf {
        Self::dir(git_dir).join(Self::file_name(kind, major, minor, patch))
    }

    /// Load the in-progress finish state for one specific source branch.
    /// Returns `None` when no finish is in progress for that branch.
    pub fn load(git_dir: &Path, kind: FinishKind, major: u32, minor: u32, patch: u32) -> Result<Option<Self>, String> {
        let path = Self::path(git_dir, kind, major, minor, patch);
        if !path.exists() {
            return Ok(None);
        }
        let contents = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
        Self::parse(&contents).map(Some)
    }

    pub fn save(&self, git_dir: &Path) -> Result<(), String> {
        let dir = Self::dir(git_dir);
        fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed to create {}: {}", dir.display(), e))?;
        let path = Self::path(git_dir, self.kind, self.major, self.minor, self.patch);
        fs::write(&path, self.serialize())
            .map_err(|e| format!("Failed to write {}: {}", path.display(), e))
    }

    /// Remove the state file for one specific source branch. Idempotent.
    pub fn clear(git_dir: &Path, kind: FinishKind, major: u32, minor: u32, patch: u32) -> Result<(), String> {
        let path = Self::path(git_dir, kind, major, minor, patch);
        if !path.exists() {
            return Ok(());
        }
        fs::remove_file(&path)
            .map_err(|e| format!("Failed to remove {}: {}", path.display(), e))
    }

    /// One-time upgrade: move a pre-2.4 global `gflow-finish.state` file into the
    /// per-branch folder under its own source-branch key. A corrupt legacy file is
    /// dropped rather than bricking startup — the finish is idempotent and can be
    /// re-driven from its source branch.
    pub fn migrate_legacy(git_dir: &Path) -> Result<(), String> {
        let legacy = git_dir.join(LEGACY_STATE_FILE_NAME);
        if !legacy.exists() {
            return Ok(());
        }
        let contents = fs::read_to_string(&legacy)
            .map_err(|e| format!("Failed to read {}: {}", legacy.display(), e))?;
        match Self::parse(&contents) {
            Ok(state) => state.save(git_dir)?,
            Err(e) => eprintln!(
                "Warning: discarding unreadable legacy finish state {}: {e}",
                legacy.display()
            ),
        }
        fs::remove_file(&legacy)
            .map_err(|e| format!("Failed to remove {}: {}", legacy.display(), e))
    }

    fn serialize(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("version={}\n", SCHEMA_VERSION));
        out.push_str(&format!("kind={}\n", self.kind.as_str()));
        out.push_str(&format!("major={}\n", self.major));
        out.push_str(&format!("minor={}\n", self.minor));
        out.push_str(&format!("patch={}\n", self.patch));
        out.push_str(&format!("started_at={}\n", self.started_at));
        if let Some(message) = &self.stash_message {
            // Key stays `stash_ref=`: unknown keys are ignored for forward
            // compatibility, so renaming it would make an older gflow silently
            // drop a stash it must restore.
            out.push_str(&format!("stash_ref={message}\n"));
        }
        out
    }

    fn parse(contents: &str) -> Result<Self, String> {
        let mut version: Option<u32> = None;
        let mut kind: Option<FinishKind> = None;
        let mut major: Option<u32> = None;
        let mut minor: Option<u32> = None;
        let mut patch: Option<u32> = None;
        let mut started_at: Option<String> = None;
        let mut stash_message: Option<String> = None;

        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, value) = line.split_once('=')
                .ok_or_else(|| format!("Malformed state line: {line}"))?;
            match key.trim() {
                "version" => version = value.trim().parse().ok(),
                "kind" => kind = FinishKind::parse(value.trim()),
                "major" => major = value.trim().parse().ok(),
                "minor" => minor = value.trim().parse().ok(),
                "patch" => patch = value.trim().parse().ok(),
                "started_at" => started_at = Some(value.trim().to_string()),
                "stash_ref" => stash_message = Some(value.trim().to_string()),
                _ => {}
            }
        }

        let version = version.ok_or("Missing version in state file")?;
        if version != SCHEMA_VERSION {
            return Err(format!(
                "Unsupported state file version {version} (expected {SCHEMA_VERSION}). \
                 Run 'gflow finish --abort' to discard."
            ));
        }
        Ok(Self {
            kind: kind.ok_or("Missing/invalid kind in state file")?,
            major: major.ok_or("Missing major in state file")?,
            minor: minor.ok_or("Missing minor in state file")?,
            patch: patch.ok_or("Missing patch in state file")?,
            started_at: started_at.ok_or("Missing started_at in state file")?,
            stash_message,
        })
    }
}

pub fn current_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir() -> PathBuf {
        crate::test_support::tmp_dir("gflow-state-test")
    }

    fn release(major: u32, minor: u32, patch: u32) -> FinishState {
        FinishState {
            kind: FinishKind::Release,
            major, minor, patch,
            started_at: "1234".to_string(),
            stash_message: None,
        }
    }

    fn hotfix(major: u32, minor: u32, patch: u32) -> FinishState {
        FinishState {
            kind: FinishKind::Hotfix,
            major, minor, patch,
            started_at: "5678".to_string(),
            stash_message: Some("gflow-finish:hotfix/2.5.2:5678".to_string()),
        }
    }

    #[test]
    fn file_name_encodes_kind_and_version() {
        assert_eq!(FinishState::file_name(FinishKind::Hotfix, 2, 5, 2), "hotfix-2.5.2.state");
        assert_eq!(FinishState::file_name(FinishKind::Release, 2, 4, 0), "release-2.4.0.state");
    }

    #[test]
    fn saves_under_per_branch_path_and_round_trips() {
        let dir = tmp_dir();
        let s = hotfix(2, 5, 2);
        s.save(&dir).unwrap();

        // File lives in the gflow-finish/ folder, keyed by the source branch.
        let expected = dir.join(STATE_DIR_NAME).join("hotfix-2.5.2.state");
        assert!(expected.exists(), "state should be saved at {}", expected.display());

        let loaded = FinishState::load(&dir, FinishKind::Hotfix, 2, 5, 2).unwrap().unwrap();
        assert_eq!(loaded, s);
        assert_eq!(loaded.source_branch(), "hotfix/2.5.2");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_returns_none_for_a_different_branch() {
        let dir = tmp_dir();
        hotfix(2, 5, 2).save(&dir).unwrap();

        // Same version, different kind -> no state for that branch.
        assert_eq!(FinishState::load(&dir, FinishKind::Release, 2, 5, 2).unwrap(), None);
        // Different version -> no state.
        assert_eq!(FinishState::load(&dir, FinishKind::Hotfix, 2, 5, 3).unwrap(), None);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn two_finishes_coexist_without_colliding() {
        let dir = tmp_dir();
        let rel = release(2, 4, 0);
        let hot = hotfix(2, 5, 2);
        rel.save(&dir).unwrap();
        hot.save(&dir).unwrap();

        assert_eq!(FinishState::load(&dir, FinishKind::Release, 2, 4, 0).unwrap().unwrap(), rel);
        assert_eq!(FinishState::load(&dir, FinishKind::Hotfix, 2, 5, 2).unwrap().unwrap(), hot);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn clear_removes_only_the_targeted_branch() {
        let dir = tmp_dir();
        release(2, 4, 0).save(&dir).unwrap();
        hotfix(2, 5, 2).save(&dir).unwrap();

        FinishState::clear(&dir, FinishKind::Hotfix, 2, 5, 2).unwrap();

        assert_eq!(FinishState::load(&dir, FinishKind::Hotfix, 2, 5, 2).unwrap(), None);
        assert!(FinishState::load(&dir, FinishKind::Release, 2, 4, 0).unwrap().is_some(),
            "clearing the hotfix must not touch the release state");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn clear_is_idempotent() {
        let dir = tmp_dir();
        FinishState::clear(&dir, FinishKind::Hotfix, 1, 0, 0).unwrap();
        FinishState::clear(&dir, FinishKind::Hotfix, 1, 0, 0).unwrap();
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unknown_keys_and_comments_are_ignored_for_forward_compatibility() {
        // decisions.md: the format has a `version=` field, and within a known
        // version "unknown keys ignored (forward-compatible)". A newer gflow that
        // adds a field must not brick an older one standing on the same repo.
        let contents = "\
# written by a newer gflow
version=1

kind=release
major=2
minor=5
patch=0
started_at=1700000000
worktree_path=/somewhere/new
";
        let state = FinishState::parse(contents).unwrap();

        assert_eq!(state.kind, FinishKind::Release);
        assert_eq!((state.major, state.minor, state.patch), (2, 5, 0));
        assert_eq!(state.stash_message, None);
    }

    #[test]
    fn an_unrecognized_kind_is_a_hard_error_not_a_default() {
        // Guessing "release" for an unknown kind would run the wrong finish flow
        // against the wrong branch. Absence is normal; malformation is not.
        let contents = "version=1\nkind=rollback\nmajor=2\nminor=5\npatch=0\nstarted_at=1\n";

        let err = FinishState::parse(contents).unwrap_err();

        assert!(err.contains("kind"), "got: {err}");
    }

    #[test]
    fn rejects_unknown_schema_version() {
        let dir = tmp_dir();
        let path = FinishState::path(&dir, FinishKind::Release, 1, 0, 0);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "version=99\nkind=release\nmajor=1\nminor=0\npatch=0\nstarted_at=0\n").unwrap();
        let err = FinishState::load(&dir, FinishKind::Release, 1, 0, 0).unwrap_err();
        assert!(err.contains("Unsupported state file version"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn migrate_legacy_moves_global_file_into_per_branch_folder() {
        let dir = tmp_dir();
        // Simulate a pre-upgrade global state file.
        let legacy = dir.join(LEGACY_STATE_FILE_NAME);
        fs::write(&legacy, release(1, 3, 0).serialize()).unwrap();

        FinishState::migrate_legacy(&dir).unwrap();

        assert!(!legacy.exists(), "legacy file should be removed after migration");
        let loaded = FinishState::load(&dir, FinishKind::Release, 1, 3, 0).unwrap().unwrap();
        assert_eq!(loaded.source_branch(), "release/1.3.0");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn migrate_legacy_is_noop_without_legacy_file() {
        let dir = tmp_dir();
        FinishState::migrate_legacy(&dir).unwrap();
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn migrate_legacy_discards_corrupt_global_file() {
        let dir = tmp_dir();
        let legacy = dir.join(LEGACY_STATE_FILE_NAME);
        fs::write(&legacy, "this is not a valid state file").unwrap();

        // A corrupt legacy file must not brick startup; it is dropped.
        FinishState::migrate_legacy(&dir).unwrap();
        assert!(!legacy.exists(), "corrupt legacy file should be removed");
        fs::remove_dir_all(&dir).ok();
    }
}

use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    Free,
    Protected,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BumpStrategy {
    Rc,
    Patch,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RepoConfig {
    pub mode: Mode,
    pub keep_release_branches: bool,
    pub bump_strategy: BumpStrategy,
}

impl Default for RepoConfig {
    fn default() -> Self {
        Self {
            mode: Mode::Free,
            keep_release_branches: false,
            bump_strategy: BumpStrategy::Rc,
        }
    }
}

pub fn parse(contents: &str) -> Result<RepoConfig, String> {
    let mut config = RepoConfig::default();

    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| format!("Malformed line in .gflow/config: {line}"))?;
        let value = value.trim();
        match key.trim() {
            "mode" => match value {
                "free" => config.mode = Mode::Free,
                "protected" => config.mode = Mode::Protected,
                _ => {
                    return Err(format!(
                        "Invalid mode '{value}' in .gflow/config. Use 'mode=free' or 'mode=protected'."
                    ));
                }
            },
            "bump-strategy" => match value {
                "rc" => config.bump_strategy = BumpStrategy::Rc,
                "patch" => config.bump_strategy = BumpStrategy::Patch,
                _ => {
                    return Err(format!(
                        "Invalid bump-strategy '{value}' in .gflow/config. Use 'bump-strategy=rc' or 'bump-strategy=patch'."
                    ));
                }
            },
            "keep-release-branches" => match value {
                "true" => config.keep_release_branches = true,
                "false" => config.keep_release_branches = false,
                _ => {
                    return Err(format!(
                        "Invalid keep-release-branches '{value}' in .gflow/config. Use 'true' or 'false'."
                    ));
                }
            },
            _ => {}
        }
    }

    Ok(config)
}

pub const NOT_INITIALISED: &str =
    "gflow is not initialised for this repository. Run 'gflow init' (interactive) and commit .gflow/config.";

fn config_path(repo_root: &Path) -> std::path::PathBuf {
    repo_root.join(".gflow").join("config")
}

pub fn exists(repo_root: &Path) -> bool {
    config_path(repo_root).exists()
}

pub fn load(repo_root: &Path) -> Result<RepoConfig, String> {
    let path = config_path(repo_root);
    if !path.exists() {
        return Err(NOT_INITIALISED.to_string());
    }
    let contents = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    parse(&contents)
}

pub fn write(repo_root: &Path, cfg: &RepoConfig) -> Result<(), String> {
    let path = config_path(repo_root);
    let dir = path.parent().expect("config path always has a parent");
    fs::create_dir_all(dir).map_err(|e| format!("Failed to create {}: {e}", dir.display()))?;
    let mode = match cfg.mode { Mode::Free => "free", Mode::Protected => "protected" };
    let strategy = match cfg.bump_strategy { BumpStrategy::Rc => "rc", BumpStrategy::Patch => "patch" };
    let contents = format!("mode={mode}\nkeep-release-branches={}\nbump-strategy={strategy}\n", cfg.keep_release_branches);
    fs::write(&path, contents).map_err(|e| format!("Failed to write {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp_dir() -> PathBuf {
        crate::test_support::tmp_dir("gflow-repo-config-test")
    }

    #[test]
    fn empty_contents_yields_default() {
        let config = parse("").unwrap();
        assert_eq!(config, RepoConfig::default());
        assert_eq!(config.mode, Mode::Free);
        assert!(!config.keep_release_branches);
    }

    #[test]
    fn mode_protected_parses() {
        let config = parse("mode=protected\n").unwrap();
        assert_eq!(config.mode, Mode::Protected);
    }

    #[test]
    fn keep_release_branches_true_parses() {
        let config = parse("keep-release-branches=true\n").unwrap();
        assert!(config.keep_release_branches);
    }

    #[test]
    fn comments_blank_lines_and_unknown_keys_are_ignored() {
        let contents = "\
# this is a comment

mode=protected
some-future-key=whatever
keep-release-branches=true
";
        let config = parse(contents).unwrap();
        assert_eq!(config.mode, Mode::Protected);
        assert!(config.keep_release_branches);
    }

    #[test]
    fn values_are_trimmed() {
        let config = parse("mode = protected \n").unwrap();
        assert_eq!(config.mode, Mode::Protected);
    }

    #[test]
    fn invalid_mode_is_a_hard_error_naming_the_remedy() {
        let err = parse("mode=banana\n").unwrap_err();
        assert!(
            err.contains("Use 'mode=free' or 'mode=protected'"),
            "got: {err}"
        );
    }

    #[test]
    fn invalid_keep_release_branches_is_a_hard_error_naming_the_remedy() {
        let err = parse("keep-release-branches=yes\n").unwrap_err();
        assert!(err.contains("Use 'true' or 'false'"), "got: {err}");
    }

    #[test]
    fn a_line_without_equals_is_malformed() {
        let err = parse("mode\n").unwrap_err();
        assert!(err.contains("Malformed line in .gflow/config"), "got: {err}");
    }

    #[test]
    fn bump_strategy_defaults_to_rc() {
        let config = parse("").unwrap();
        assert_eq!(config.bump_strategy, BumpStrategy::Rc);
    }

    #[test]
    fn bump_strategy_patch_parses() {
        let config = parse("bump-strategy=patch\n").unwrap();
        assert_eq!(config.bump_strategy, BumpStrategy::Patch);
    }

    #[test]
    fn bump_strategy_rc_parses_explicitly() {
        let config = parse("bump-strategy=rc\n").unwrap();
        assert_eq!(config.bump_strategy, BumpStrategy::Rc);
    }

    #[test]
    fn invalid_bump_strategy_is_a_hard_error_naming_the_remedy() {
        let err = parse("bump-strategy=banana\n").unwrap_err();
        assert!(
            err.contains("Use 'bump-strategy=rc' or 'bump-strategy=patch'"),
            "got: {err}"
        );
    }

    #[test]
    fn load_round_trips_through_the_filesystem() {
        let dir = tmp_dir();
        let gflow_dir = dir.join(".gflow");
        fs::create_dir_all(&gflow_dir).unwrap();
        fs::write(
            gflow_dir.join("config"),
            "mode=protected\nkeep-release-branches=true\n",
        )
        .unwrap();

        let config = load(&dir).unwrap();

        assert_eq!(
            config,
            RepoConfig {
                mode: Mode::Protected,
                keep_release_branches: true,
                bump_strategy: BumpStrategy::Rc,
            }
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_on_a_root_with_no_gflow_dir_is_not_initialised() {
        let dir = tmp_dir();
        assert!(!exists(&dir));
        assert_eq!(load(&dir).unwrap_err(), NOT_INITIALISED);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_then_load_round_trips_every_key() {
        let dir = tmp_dir();
        let cfg = RepoConfig { mode: Mode::Protected, keep_release_branches: true, bump_strategy: BumpStrategy::Patch };
        write(&dir, &cfg).unwrap();
        assert!(exists(&dir));
        assert_eq!(fs::read_to_string(dir.join(".gflow").join("config")).unwrap(),
            "mode=protected\nkeep-release-branches=true\nbump-strategy=patch\n");
        assert_eq!(load(&dir).unwrap(), cfg);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_serialises_the_defaults() {
        let dir = tmp_dir();
        write(&dir, &RepoConfig::default()).unwrap();
        assert_eq!(fs::read_to_string(dir.join(".gflow").join("config")).unwrap(),
            "mode=free\nkeep-release-branches=false\nbump-strategy=rc\n");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_fails_when_the_root_is_not_a_directory() {
        let dir = tmp_dir();
        let not_a_dir = dir.join("file");
        fs::write(&not_a_dir, "x").unwrap();
        let err = write(&not_a_dir, &RepoConfig::default()).unwrap_err();
        assert!(err.starts_with("Failed to create"), "got: {err}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_fails_when_config_is_a_directory() {
        let dir = tmp_dir();
        fs::create_dir_all(dir.join(".gflow").join("config")).unwrap();
        let err = load(&dir).unwrap_err();
        assert!(err.starts_with("Failed to read"), "got: {err}");
        fs::remove_dir_all(&dir).ok();
    }
}

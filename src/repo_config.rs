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

/// One configuration layer: every key is `None` until that layer sets it.
///
/// Layers are merged before defaults are applied, so a global `mode=protected`
/// is not silently undone by a repo file that simply does not mention `mode`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Settings {
    pub mode: Option<Mode>,
    pub keep_release_branches: Option<bool>,
    pub bump_strategy: Option<BumpStrategy>,
    pub worktree: Option<bool>,
    pub editor: Option<String>,
    pub path: Option<String>,
}

impl Settings {
    /// Lay `over` on top of `self`: every key `over` sets wins, every key it
    /// leaves unset falls through. Silence is never an override.
    pub fn merge(self, over: Settings) -> Settings {
        Settings {
            mode: over.mode.or(self.mode),
            keep_release_branches: over.keep_release_branches.or(self.keep_release_branches),
            bump_strategy: over.bump_strategy.or(self.bump_strategy),
            worktree: over.worktree.or(self.worktree),
            editor: over.editor.or(self.editor),
            path: over.path.or(self.path),
        }
    }

    /// Collapse a merged layer stack into the values the flows actually take.
    pub fn resolve(self) -> RepoConfig {
        let defaults = RepoConfig::default();
        RepoConfig {
            mode: self.mode.unwrap_or(defaults.mode),
            keep_release_branches: self.keep_release_branches.unwrap_or(defaults.keep_release_branches),
            bump_strategy: self.bump_strategy.unwrap_or(defaults.bump_strategy),
        }
    }
}

pub fn parse(contents: &str) -> Result<Settings, String> {
    let mut config = Settings::default();

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
                "free" => config.mode = Some(Mode::Free),
                "protected" => config.mode = Some(Mode::Protected),
                _ => {
                    return Err(format!(
                        "Invalid mode '{value}' in .gflow/config. Use 'mode=free' or 'mode=protected'."
                    ));
                }
            },
            "bump-strategy" => match value {
                "rc" => config.bump_strategy = Some(BumpStrategy::Rc),
                "patch" => config.bump_strategy = Some(BumpStrategy::Patch),
                _ => {
                    return Err(format!(
                        "Invalid bump-strategy '{value}' in .gflow/config. Use 'bump-strategy=rc' or 'bump-strategy=patch'."
                    ));
                }
            },
            "keep-release-branches" => match value {
                "true" => config.keep_release_branches = Some(true),
                "false" => config.keep_release_branches = Some(false),
                _ => {
                    return Err(format!(
                        "Invalid keep-release-branches '{value}' in .gflow/config. Use 'true' or 'false'."
                    ));
                }
            },
            "worktree" => match value {
                "true" => config.worktree = Some(true),
                "false" => config.worktree = Some(false),
                _ => {
                    return Err(format!(
                        "Invalid worktree '{value}' in .gflow/config. Use 'worktree=true' or 'worktree=false'."
                    ));
                }
            },
            "editor" => config.editor = Some(value.to_string()),
            "path" => config.path = Some(value.to_string()),
            _ => {}
        }
    }

    Ok(config)
}

pub const LOCAL_FILE_NAME: &str = "config.local";

/// Parse the gitignored per-repo override. Team policy is not overridable
/// privately — a developer opting out of `mode=protected` in a file nobody
/// reviews would defeat the guarantee the committed file exists to make.
/// Team keys found here are dropped and reported, never applied.
pub fn parse_local(contents: &str) -> Result<(Settings, Vec<String>), String> {
    let mut settings = parse(contents)?;
    let mut warnings = Vec::new();
    let mut drop = |present: bool, key: &str| {
        if present {
            warnings.push(format!(
                "'{key}' is a team setting and is ignored in .gflow/{LOCAL_FILE_NAME}. Move it to .gflow/config (gflow worktree ... --repo)."
            ));
        }
    };
    drop(settings.mode.is_some(), "mode");
    drop(settings.keep_release_branches.is_some(), "keep-release-branches");
    drop(settings.bump_strategy.is_some(), "bump-strategy");
    settings.mode = None;
    settings.keep_release_branches = None;
    settings.bump_strategy = None;
    Ok((settings, warnings))
}

/// Keep the private override out of git. Written inside `.gflow/` so gflow
/// never edits the repository's own `.gitignore`, and self-ignoring so gflow's
/// bookkeeping is not untracked noise in a repo that shares no config.
pub fn ensure_local_gitignored(gflow_dir: &Path) -> Result<(), String> {
    let path = gflow_dir.join(".gitignore");
    if path.exists() {
        return Ok(());
    }
    fs::create_dir_all(gflow_dir)
        .map_err(|e| format!("Failed to create {}: {e}", gflow_dir.display()))?;
    fs::write(&path, format!("{LOCAL_FILE_NAME}\n.gitignore\n"))
        .map_err(|e| format!("Failed to write {}: {e}", path.display()))
}

pub fn local_config_path(repo_root: &Path) -> std::path::PathBuf {
    repo_root.join(".gflow").join(LOCAL_FILE_NAME)
}

pub const NOT_INITIALISED: &str =
    "gflow is not initialised for this repository. Run 'gflow init' (interactive) and commit .gflow/config.";

pub fn config_path(repo_root: &Path) -> std::path::PathBuf {
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
    parse(&contents).map(Settings::resolve)
}

/// What the layer stack resolved to. `initialised` is false when no layer had
/// a config file at all — whether that is an error is the caller's policy, not
/// the loader's (`gflow worktree status` must still render the defaults).
#[derive(Debug, Default)]
pub struct Layers {
    pub settings: Settings,
    pub warnings: Vec<String>,
    pub initialised: bool,
}

/// Read one layer. A missing file is an empty layer, not an error.
fn read_layer(path: &std::path::PathBuf) -> Result<Option<String>, String> {
    if !path.exists() {
        return Ok(None);
    }
    fs::read_to_string(path)
        .map(Some)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))
}

/// Resolve the layer stack for `repo_root`, lowest first:
/// `~/.gflow/config` (yours, every repo) → `<repo>/.gflow/config` (the repo's,
/// committed) → `<repo>/.gflow/config.local` (yours, this repo, gitignored).
///
/// Not-initialised is still "no config file anywhere", exactly as before the
/// global layer existed — a repo whose committed file omits a key keeps
/// falling back to the built-in default rather than becoming an error.
pub fn load_layers(home: Option<&Path>, repo_root: Option<&Path>) -> Result<Layers, String> {
    let global_contents = match home.map(global_config_path) {
        Some(path) => read_layer(&path)?,
        None => None,
    };
    let (repo_contents, local_contents) = match repo_root {
        Some(root) => (
            read_layer(&config_path(root))?,
            read_layer(&local_config_path(root))?,
        ),
        None => (None, None),
    };
    let initialised =
        global_contents.is_some() || repo_contents.is_some() || local_contents.is_some();

    let mut settings = Settings::default();
    for contents in [&global_contents, &repo_contents].into_iter().flatten() {
        settings = settings.merge(parse(contents)?);
    }
    let mut warnings = Vec::new();
    if let Some(contents) = &local_contents {
        let (local_settings, local_warnings) = parse_local(contents)?;
        settings = settings.merge(local_settings);
        warnings = local_warnings;
    }
    Ok(Layers { settings, warnings, initialised })
}

pub fn global_config_path(home: &Path) -> std::path::PathBuf {
    home.join(".gflow").join("config")
}

/// Set one key in a config file, or remove it when `value` is `None` ("use the
/// default"). Rewrites the key in place and leaves every other line — comments
/// included — untouched, because these files are hand-edited and committed.
pub fn set_key(path: &Path, key: &str, value: Option<&str>) -> Result<(), String> {
    let existing = read_layer(&path.to_path_buf())?.unwrap_or_default();
    let mut out = String::new();
    let mut replaced = false;
    for line in existing.lines() {
        if line.split_once('=').map(|(k, _)| k.trim() == key).unwrap_or(false) {
            if let Some(value) = value {
                out.push_str(&format!("{key}={value}\n"));
            }
            replaced = true;
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    if !replaced {
        if let Some(value) = value {
            out.push_str(&format!("{key}={value}\n"));
        }
    }
    let dir = path.parent().expect("config path always has a parent");
    fs::create_dir_all(dir).map_err(|e| format!("Failed to create {}: {e}", dir.display()))?;
    fs::write(path, out).map_err(|e| format!("Failed to write {}: {e}", path.display()))
}

/// The 4.0.x git config keys, paired with the file key that replaces them.
/// `gflow.branch.main` and `gflow.hosting.provider` are absent on purpose:
/// they are detection caches gflow writes back itself, not settings a user
/// chose, so they stay in git config where their per-clone lifetime belongs.
type MigratedKey = (&'static str, &'static str, fn(&Settings) -> bool);
const MIGRATED_KEYS: &[MigratedKey] = &[
    ("gflow.worktree.enabled", "worktree", |s| s.worktree.is_some()),
    ("gflow.worktree.editor", "editor", |s| s.editor.is_some()),
    ("gflow.worktree.path", "path", |s| s.path.is_some()),
];

/// Move any 4.0.x `gflow.worktree.*` git config into the layered files, each
/// key landing in the scope it was set in: global git config → `~/.gflow/config`,
/// local → `<repo>/.gflow/config.local`. Local git config was never committed,
/// so migrating it into the *committed* file would publish a personal setting.
///
/// Idempotent by construction — the git key is unset once its value is
/// accounted for, so the next run finds nothing and writes nothing.
pub fn migrate_git_config(
    git: &dyn crate::git::Git,
    home: Option<&Path>,
    repo_root: Option<&Path>,
) -> Result<(), String> {
    if let Some(home) = home {
        migrate_scope(git, true, &global_config_path(home))?;
    }
    if let Some(repo_root) = repo_root {
        migrate_scope(git, false, &local_config_path(repo_root))?;
    }
    Ok(())
}

fn migrate_scope(
    git: &dyn crate::git::Git,
    global: bool,
    target: &std::path::PathBuf,
) -> Result<(), String> {
    let existing = read_layer(target)?;
    let stated = parse(existing.as_deref().unwrap_or(""))?;
    let mut added = String::new();

    for (git_key, file_key, already_stated) in MIGRATED_KEYS {
        let Some(value) = git.get_config_at(git_key, global)? else {
            continue;
        };
        // The file already states this key: the file wins, the stale git key
        // still goes, so a later run cannot resurrect the old value.
        if !already_stated(&stated) {
            if let Some(line) = migrated_line(file_key, value.trim()) {
                added.push_str(&line);
            }
        }
        git.unset_config(git_key, global)?;
    }

    if added.is_empty() {
        return Ok(());
    }
    let dir = target.parent().expect("config path always has a parent");
    fs::create_dir_all(dir).map_err(|e| format!("Failed to create {}: {e}", dir.display()))?;
    let mut contents = existing.unwrap_or_default();
    if !contents.is_empty() && !contents.ends_with('\n') {
        contents.push('\n');
    }
    contents.push_str(&added);
    if !global {
        ensure_local_gitignored(dir)?;
    }
    fs::write(target, contents)
        .map_err(|e| format!("Failed to write {}: {e}", target.display()))
}

/// The user's home directory, or `None` on a machine that states neither
/// `HOME` nor `USERPROFILE` (a bare container).
pub fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
}

/// git config accepted any casing for the boolean and any surrounding
/// whitespace; the file format does not. Normalise rather than write a line
/// the parser would then reject.
fn migrated_line(file_key: &str, value: &str) -> Option<String> {
    match file_key {
        "worktree" => Some(format!("worktree={}\n", value.eq_ignore_ascii_case("true"))),
        _ if value.is_empty() => None,
        _ => Some(format!("{file_key}={value}\n")),
    }
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
    fn empty_contents_sets_no_key() {
        assert_eq!(parse("").unwrap(), Settings::default());
    }

    #[test]
    fn a_key_the_layer_omits_stays_none_so_a_lower_layer_can_supply_it() {
        let settings = parse("mode=protected\n").unwrap();
        assert_eq!(settings.mode, Some(Mode::Protected));
        assert_eq!(settings.keep_release_branches, None);
        assert_eq!(settings.bump_strategy, None);
    }

    #[test]
    fn the_personal_keys_parse() {
        let settings = parse("worktree=true\neditor=zed\npath=~/wt\n").unwrap();
        assert_eq!(settings.worktree, Some(true));
        assert_eq!(settings.editor.as_deref(), Some("zed"));
        assert_eq!(settings.path.as_deref(), Some("~/wt"));
    }

    #[test]
    fn invalid_worktree_is_a_hard_error_naming_the_remedy() {
        let err = parse("worktree=yes\n").unwrap_err();
        assert!(err.contains("Use 'worktree=true' or 'worktree=false'"), "got: {err}");
    }

    #[test]
    fn a_higher_layer_overrides_only_the_keys_it_sets() {
        let global = parse("mode=protected\neditor=zed\n").unwrap();
        let repo = parse("editor=code\n").unwrap();

        let merged = global.merge(repo);

        assert_eq!(merged.editor.as_deref(), Some("code"), "repo layer wins");
        assert_eq!(merged.mode, Some(Mode::Protected), "global survives a silent repo layer");
    }

    #[test]
    fn an_empty_higher_layer_changes_nothing() {
        let global = parse("mode=protected\nworktree=true\n").unwrap();
        assert_eq!(global.clone().merge(Settings::default()), global);
    }

    fn write_config(dir: &Path, name: &str, contents: &str) {
        let gflow = dir.join(".gflow");
        fs::create_dir_all(&gflow).unwrap();
        fs::write(gflow.join(name), contents).unwrap();
    }

    #[test]
    fn config_local_keeps_the_personal_keys() {
        let (settings, warnings) = parse_local("worktree=true\neditor=zed\npath=~/wt\n").unwrap();
        assert_eq!(settings.worktree, Some(true));
        assert_eq!(settings.editor.as_deref(), Some("zed"));
        assert_eq!(settings.path.as_deref(), Some("~/wt"));
        assert!(warnings.is_empty(), "got: {warnings:?}");
    }

    #[test]
    fn config_local_cannot_opt_out_of_the_teams_landing_mode() {
        let (settings, warnings) = parse_local("mode=free\nworktree=true\n").unwrap();
        assert_eq!(settings.mode, None, "mode must not be overridable privately");
        assert_eq!(settings.worktree, Some(true), "the personal key still applies");
        assert!(warnings.iter().any(|w| w.contains("mode")), "got: {warnings:?}");
    }

    #[test]
    fn config_local_ignores_every_team_key() {
        let (settings, warnings) =
            parse_local("keep-release-branches=true\nbump-strategy=patch\n").unwrap();
        assert_eq!(settings.keep_release_branches, None);
        assert_eq!(settings.bump_strategy, None);
        assert_eq!(warnings.len(), 2, "got: {warnings:?}");
    }

    #[test]
    fn config_local_overrides_both_lower_layers() {
        let home = tmp_dir();
        let repo = tmp_dir();
        write_config(&home, "config", "worktree=false\n");
        write_config(&repo, "config", "mode=protected\nworktree=true\n");
        write_config(&repo, LOCAL_FILE_NAME, "worktree=false\neditor=zed\n");

        let layers = load_layers(Some(&home), Some(&repo)).unwrap();

        assert_eq!(layers.settings.worktree, Some(false), "my private opt-out wins");
        assert_eq!(layers.settings.editor.as_deref(), Some("zed"));
        assert_eq!(layers.settings.mode, Some(Mode::Protected), "team policy survives");
        fs::remove_dir_all(&home).ok();
        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn config_local_warnings_reach_the_caller() {
        let repo = tmp_dir();
        write_config(&repo, "config", "mode=protected\n");
        write_config(&repo, LOCAL_FILE_NAME, "mode=free\n");

        let layers = load_layers(None, Some(&repo)).unwrap();

        assert_eq!(layers.settings.mode, Some(Mode::Protected));
        assert_eq!(layers.warnings.len(), 1, "got: {:?}", layers.warnings);
        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn a_repo_with_only_a_private_override_is_still_initialised() {
        let repo = tmp_dir();
        write_config(&repo, LOCAL_FILE_NAME, "worktree=true\n");
        assert!(load_layers(None, Some(&repo)).unwrap().initialised);
        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn ensure_local_gitignored_is_idempotent_and_never_clobbers() {
        let dir = tmp_dir();
        let gflow = dir.join(".gflow");
        ensure_local_gitignored(&gflow).unwrap();
        // Self-ignoring: gflow's own bookkeeping must not turn up as untracked
        // noise in a repo the user never asked to share config from.
        assert_eq!(
            fs::read_to_string(gflow.join(".gitignore")).unwrap(),
            "config.local\n.gitignore\n"
        );

        fs::write(gflow.join(".gitignore"), "config.local\n*.log\n").unwrap();
        ensure_local_gitignored(&gflow).unwrap();

        assert_eq!(
            fs::read_to_string(gflow.join(".gitignore")).unwrap(),
            "config.local\n*.log\n",
            "an existing ignore file is left alone"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_repo_with_no_config_falls_back_to_the_global_file() {
        let home = tmp_dir();
        let repo = tmp_dir();
        write_config(&home, "config", "mode=protected\nworktree=true\n");

        let settings = load_layers(Some(&home), Some(&repo)).unwrap().settings;

        assert_eq!(settings.mode, Some(Mode::Protected));
        assert_eq!(settings.worktree, Some(true));
        fs::remove_dir_all(&home).ok();
        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn the_committed_repo_file_overrides_the_global_file() {
        let home = tmp_dir();
        let repo = tmp_dir();
        write_config(&home, "config", "mode=free\nbump-strategy=patch\n");
        write_config(&repo, "config", "mode=protected\n");

        let settings = load_layers(Some(&home), Some(&repo)).unwrap().settings;

        assert_eq!(settings.mode, Some(Mode::Protected), "repo beats global");
        assert_eq!(settings.bump_strategy, Some(BumpStrategy::Patch), "global still supplies the rest");
        fs::remove_dir_all(&home).ok();
        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn no_config_file_in_any_layer_reports_not_initialised() {
        let home = tmp_dir();
        let repo = tmp_dir();
        assert!(!load_layers(Some(&home), Some(&repo)).unwrap().initialised);
        fs::remove_dir_all(&home).ok();
        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn an_unknown_home_is_simply_an_empty_layer() {
        let repo = tmp_dir();
        write_config(&repo, "config", "mode=protected\n");
        let settings = load_layers(None, Some(&repo)).unwrap().settings;
        assert_eq!(settings.mode, Some(Mode::Protected));
        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn set_key_creates_the_file_and_its_directory() {
        let dir = tmp_dir();
        let path = dir.join(".gflow").join("config");
        set_key(&path, "worktree", Some("true")).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "worktree=true\n");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn set_key_replaces_a_key_in_place_and_keeps_the_rest() {
        let dir = tmp_dir();
        let path = dir.join("config");
        fs::write(&path, "# mine\nmode=protected\neditor=code\npath=~/wt\n").unwrap();

        set_key(&path, "editor", Some("zed")).unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "# mine\nmode=protected\neditor=zed\npath=~/wt\n"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn set_key_with_no_value_removes_the_line_reverting_to_the_default() {
        let dir = tmp_dir();
        let path = dir.join("config");
        fs::write(&path, "mode=protected\npath=~/wt\n").unwrap();

        set_key(&path, "path", None).unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "mode=protected\n");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn set_key_appends_a_key_the_file_does_not_have_yet() {
        let dir = tmp_dir();
        let path = dir.join("config");
        fs::write(&path, "mode=protected\n").unwrap();

        set_key(&path, "worktree", Some("true")).unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "mode=protected\nworktree=true\n");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn outside_a_repository_only_the_global_layer_is_read() {
        let home = tmp_dir();
        write_config(&home, "config", "worktree=true\n");
        let layers = load_layers(Some(&home), None).unwrap();
        assert!(layers.initialised);
        assert_eq!(layers.settings.worktree, Some(true));
        fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn home_dir_reads_the_environment() {
        // The one caller that cannot be injected: the composition root has to
        // find the home directory before any config exists to point at it.
        // HOME wins where both are set (Unix CI); USERPROFILE is the fallback
        // a bare HOME-less machine (Windows CI) actually has.
        let expected = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(std::path::PathBuf::from);
        assert_eq!(home_dir(), expected);
    }

    #[test]
    fn set_key_terminates_a_file_that_had_no_trailing_newline() {
        let dir = tmp_dir();
        let path = dir.join("config");
        fs::write(&path, "mode=protected").unwrap();
        set_key(&path, "worktree", Some("true")).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "mode=protected\nworktree=true\n");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn empty_contents_yields_default() {
        let config = parse("").unwrap().resolve();
        assert_eq!(config, RepoConfig::default());
        assert_eq!(config.mode, Mode::Free);
        assert!(!config.keep_release_branches);
    }

    #[test]
    fn mode_protected_parses() {
        let config = parse("mode=protected\n").unwrap().resolve();
        assert_eq!(config.mode, Mode::Protected);
    }

    #[test]
    fn keep_release_branches_true_parses() {
        let config = parse("keep-release-branches=true\n").unwrap().resolve();
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
        let config = parse(contents).unwrap().resolve();
        assert_eq!(config.mode, Mode::Protected);
        assert!(config.keep_release_branches);
    }

    #[test]
    fn values_are_trimmed() {
        let config = parse("mode = protected \n").unwrap().resolve();
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
        let config = parse("").unwrap().resolve();
        assert_eq!(config.bump_strategy, BumpStrategy::Rc);
    }

    #[test]
    fn bump_strategy_patch_parses() {
        let config = parse("bump-strategy=patch\n").unwrap().resolve();
        assert_eq!(config.bump_strategy, BumpStrategy::Patch);
    }

    #[test]
    fn bump_strategy_rc_parses_explicitly() {
        let config = parse("bump-strategy=rc\n").unwrap().resolve();
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

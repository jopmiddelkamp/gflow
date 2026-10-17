//! Event hooks: commands a repo declares in `.gflow/config` (`hook-<event>=`)
//! and gflow runs after a tag event is complete.
//!
//! Delivery is at most once per command, event and tag on this clone. Each
//! event gets a ledger file under the shared `.git` directory: written
//! `pending` when the flow reports the event, each command's digest written
//! before that command starts, `done` once every command was handled. Any
//! later hook-configured run fires what is still pending, so a crash never
//! loses or repeats a command. Approvals live per user, outside the repo,
//! because a pull request can change the commands.

use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::git::Git;
use crate::prompt::Prompter;
use crate::sha256::hex_digest;
use crate::version::SemVer;

/// gflow pushes every tag to `origin`.
pub const REMOTE: &str = "origin";
/// Folder under the shared `.git` directory holding one ledger per event and tag.
pub const LEDGER_DIR: &str = "gflow-hooks";
/// File under `~/.gflow/` holding the approved commands, one `<sha256> <repo root>` per line.
pub const APPROVALS_FILE: &str = "hook-approvals";
const LEDGER_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEvent {
    ReleaseStart,
    ReleaseBump,
    ReleaseFinish,
    HotfixFinish,
}

impl HookEvent {
    pub const ALL: [HookEvent; 4] = [Self::ReleaseStart, Self::ReleaseBump, Self::ReleaseFinish, Self::HotfixFinish];

    pub fn name(self) -> &'static str {
        match self {
            Self::ReleaseStart => "release-start",
            Self::ReleaseBump => "release-bump",
            Self::ReleaseFinish => "release-finish",
            Self::HotfixFinish => "hotfix-finish",
        }
    }

    pub fn key(self) -> String {
        format!("hook-{}", self.name())
    }

    fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.name() == name)
    }
}

/// One `hook-*` line of `.gflow/config`. A typo must not silently disable a
/// deploy, so an unknown `hook-*` key is an error, unlike other unknown keys.
pub fn parse_entry(key: &str, value: &str) -> Result<(HookEvent, String), String> {
    let event = HookEvent::ALL.into_iter().find(|e| e.key() == key).ok_or_else(|| {
        let valid: Vec<String> = HookEvent::ALL.iter().map(|e| e.key()).collect();
        format!("Unknown key '{key}' in .gflow/config. Valid hook keys: {}.", valid.join(", "))
    })?;
    let command = value.trim();
    if command.is_empty() {
        return Err(format!("Empty command for '{key}' in .gflow/config."));
    }
    Ok((event, command.to_string()))
}

/// A completed tag event, as a flow reports it.
#[derive(Debug, Clone, PartialEq)]
pub struct TagEvent {
    pub event: HookEvent,
    pub tag: SemVer,
    pub branch: String,
}

impl TagEvent {
    pub fn new(event: HookEvent, tag: &SemVer, branch: &str) -> Self {
        Self { event, tag: tag.clone(), branch: branch.to_string() }
    }

    pub fn env(&self) -> Vec<(&'static str, String)> {
        let kind = if self.tag.is_pre_release() { "rc" } else { "final" };
        vec![
            ("GFLOW_EVENT", self.event.name().to_string()),
            ("GFLOW_TAG", self.tag.tag_name()),
            ("GFLOW_TAG_KIND", kind.to_string()),
            ("GFLOW_BRANCH", self.branch.clone()),
            ("GFLOW_REMOTE", REMOTE.to_string()),
        ]
    }
}

/// The exact command line that repeats one hook run by hand.
pub fn rerun_line(command: &str, cwd: &Path, env: &[(&'static str, String)], windows: bool) -> String {
    if windows {
        let sets: Vec<String> = env.iter().map(|(k, v)| format!("set \"{k}={v}\" && ")).collect();
        format!("cd /d \"{}\" && {}cmd /C \"{command}\"", cwd.display(), sets.concat())
    } else {
        let quote = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
        let vars: Vec<String> = env.iter().map(|(k, v)| format!("{k}={} ", quote(v))).collect();
        format!("cd {} && {}sh -c {}", quote(&cwd.display().to_string()), vars.concat(), quote(command))
    }
}

/// Empty for a success; otherwise one line with the outcome and the rerun command.
pub fn failure_line(outcome: &Result<Option<i32>, String>, command: &str, cwd: &Path, env: &[(&'static str, String)], windows: bool) -> String {
    let why = match outcome {
        Ok(Some(0)) => return String::new(),
        Ok(Some(code)) => format!("exit {code}"),
        Ok(None) => "killed by a signal".to_string(),
        Err(e) => e.clone(),
    };
    format!("✖ Hook failed ({why}). Rerun: {}", rerun_line(command, cwd, env, windows))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    AlreadyRan,
    Run,
    Ask,
    SkipUnapproved,
}

/// What to do with one command of a pending event.
pub fn decide(already_ran: bool, approved: bool, interactive: bool) -> Step {
    match (already_ran, approved, interactive) {
        (true, _, _) => Step::AlreadyRan,
        (false, true, _) => Step::Run,
        (false, false, true) => Step::Ask,
        (false, false, false) => Step::SkipUnapproved,
    }
}

/// Port: run one hook command through the platform shell.
/// `Ok(Some(code))` = exited, `Ok(None)` = killed by a signal, `Err` = no shell.
pub trait HookShell {
    fn run(&self, command: &str, cwd: &Path, env: &[(&'static str, String)]) -> Result<Option<i32>, String>;
}

/// Production `HookShell`: a zero-policy shell like `ShellSetup`.
pub struct ShellHook;

impl HookShell for ShellHook {
    fn run(&self, command: &str, cwd: &Path, env: &[(&'static str, String)]) -> Result<Option<i32>, String> {
        let env: Vec<(&str, &OsStr)> = env.iter().map(|(k, v)| (*k, OsStr::new(v.as_str()))).collect();
        crate::shell::status(command, cwd, &env)
            .map(|status| status.code())
            .map_err(|e| format!("could not start the shell: {e}"))
    }
}

pub fn is_approved(file: &Path, repo_root: &Path, command: &str) -> Result<bool, String> {
    if !file.exists() {
        return Ok(false);
    }
    let contents = fs::read_to_string(file).map_err(|e| format!("Failed to read {}: {e}", file.display()))?;
    let line = approval_line(repo_root, command);
    Ok(contents.lines().any(|l| l == line))
}

pub fn approve(file: &Path, repo_root: &Path, command: &str) -> Result<(), String> {
    let dir = file.parent().expect("the approvals file always sits in ~/.gflow");
    fs::create_dir_all(dir)
        .and_then(|_| fs::OpenOptions::new().create(true).append(true).open(file))
        .and_then(|mut f| f.write_all(format!("{}\n", approval_line(repo_root, command)).as_bytes()))
        .map_err(|e| format!("Failed to write {}: {e}", file.display()))
}

fn approval_line(repo_root: &Path, command: &str) -> String {
    format!("{} {}", hex_digest(command.as_bytes()), repo_root.display())
}

/// What `main.rs` builds: the commands, the shell, whether a human can answer, where approvals live.
pub struct HookEnv<'a> {
    /// The committed file's `hook-*` lines, in file order.
    pub commands: Vec<(HookEvent, String)>,
    pub shell: &'a dyn HookShell,
    pub interactive: bool,
    pub approvals_file: Option<PathBuf>,
}

/// The hooks of one run, resolved before the flow: a finish can remove the
/// worktree this process stands in, after which no git call can run.
pub struct Hooks<'a> {
    ledger_dir: PathBuf,
    repo_root: PathBuf,
    env: &'a HookEnv<'a>,
}

impl<'a> Hooks<'a> {
    /// `None` when the repo configures no hooks: no git call, no output.
    pub fn prepare(git: &dyn Git, env: &'a HookEnv<'a>) -> Result<Option<Self>, String> {
        if env.commands.is_empty() {
            return Ok(None);
        }
        let repo_root = git.repo_root()?;
        let ledger_dir = git.git_common_dir()?.join(LEDGER_DIR);
        Ok(Some(Self { ledger_dir, repo_root, env }))
    }

    /// Persist a completed event before anything else happens. Recording an
    /// event twice keeps the first record. With `no_hooks` the event is
    /// recorded as handled and its commands are listed, not run.
    pub fn record(&self, event: &TagEvent, no_hooks: bool) -> Result<(), String> {
        let path = self.ledger_dir.join(Ledger::file_name(event));
        if path.exists() {
            return Ok(());
        }
        let ledger = Ledger { event: event.clone(), recorded_at: now_nanos(), done: no_hooks, ran: Vec::new() };
        if no_hooks {
            for command in self.commands_for(event.event) {
                println!("↷ Hook skipped (--no-hooks): {}", rerun_line(command, &self.repo_root, &event.env(), cfg!(windows)));
            }
        }
        ledger.save(&path)
    }

    /// Run every command of every pending event, oldest event first.
    pub fn fire_pending(&self, prompter: &dyn Prompter) -> Result<(), String> {
        let mut failures = Vec::new();
        for (path, mut ledger) in Ledger::pending(&self.ledger_dir)? {
            for command in self.commands_for(ledger.event.event) {
                let digest = hex_digest(command.as_bytes());
                if self.should_run(&ledger, &digest, command, prompter)? {
                    ledger.ran.push(digest);
                    ledger.save(&path)?;
                    let env = ledger.event.env();
                    let outcome = self.env.shell.run(command, &self.repo_root, &env);
                    let line = failure_line(&outcome, command, &self.repo_root, &env, cfg!(windows));
                    if !line.is_empty() {
                        eprintln!("{line}");
                        failures.push(ledger.event.clone());
                    }
                }
            }
            ledger.done = true;
            ledger.save(&path)?;
        }
        match failures.as_slice() {
            [] => Ok(()),
            [first, ..] => {
                let noun = if failures.len() == 1 { "command" } else { "commands" };
                Err(format!(
                    "{} hook {noun} failed. The {} of {} itself is complete; rerun each failed command shown above.",
                    failures.len(), first.event.name(), first.tag.tag_name(),
                ))
            }
        }
    }

    fn should_run(&self, ledger: &Ledger, digest: &str, command: &str, prompter: &dyn Prompter) -> Result<bool, String> {
        let approved = match &self.env.approvals_file {
            Some(file) => is_approved(file, &self.repo_root, command)?,
            None => false,
        };
        match decide(ledger.ran.iter().any(|d| d == digest), approved, self.env.interactive) {
            Step::AlreadyRan => Ok(false),
            Step::Run => Ok(true),
            Step::SkipUnapproved => {
                eprintln!("↷ Hook not run: not approved, and no terminal to ask. Run gflow in a terminal to approve: {command}");
                Ok(false)
            }
            Step::Ask => {
                let answer = prompter.prompt_line(&format!("Hook for {}: {command}\nRun this hook? [y/N]", ledger.event.event.name()))?;
                let yes = matches!(answer.to_lowercase().as_str(), "y" | "yes");
                if yes {
                    if let Some(file) = &self.env.approvals_file {
                        approve(file, &self.repo_root, command)?;
                    }
                }
                Ok(yes)
            }
        }
    }

    fn commands_for(&self, event: HookEvent) -> impl Iterator<Item = &String> {
        self.env.commands.iter().filter(move |(e, _)| *e == event).map(|(_, c)| c)
    }
}

/// Nanoseconds, not `current_timestamp`'s seconds: one run can record two
/// events (a leftover and its own), and they must fire in that order.
fn now_nanos() -> String {
    let since_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is after 1970");
    since_epoch.as_nanos().to_string()
}

/// One event's delivery record. Versioned `key=value`, like `FinishState`.
#[derive(Debug, Clone, PartialEq)]
struct Ledger {
    event: TagEvent,
    recorded_at: String,
    done: bool,
    ran: Vec<String>,
}

impl Ledger {
    fn file_name(event: &TagEvent) -> String {
        format!("{}-{}.state", event.event.name(), event.tag.tag_name())
    }

    /// Every not-yet-done ledger, oldest first. A missing folder is none.
    fn pending(dir: &Path) -> Result<Vec<(PathBuf, Ledger)>, String> {
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let paths = fs::read_dir(dir)
            .and_then(|entries| entries.map(|entry| entry.map(|e| e.path())).collect::<std::io::Result<Vec<_>>>())
            .map_err(|e| format!("Failed to read {}: {e}", dir.display()))?;
        let mut pending = Vec::new();
        for path in paths {
            let contents = fs::read_to_string(&path).map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
            let ledger = Self::parse(&contents).map_err(|e| {
                format!("Corrupt hook ledger {}: {e}. Delete it to continue (its commands will not run again).", path.display())
            })?;
            if !ledger.done {
                pending.push((path, ledger));
            }
        }
        pending.sort_by(|(pa, a), (pb, b)| (a.recorded_at.parse::<u128>().unwrap_or(0), pa).cmp(&(b.recorded_at.parse::<u128>().unwrap_or(0), pb)));
        Ok(pending)
    }

    fn save(&self, path: &Path) -> Result<(), String> {
        let dir = path.parent().expect("a ledger always sits in the ledger folder");
        fs::create_dir_all(dir)
            .and_then(|_| fs::write(path, self.serialize()))
            .map_err(|e| format!("Failed to write {}: {e}", path.display()))
    }

    fn serialize(&self) -> String {
        let mut out = format!(
            "version={LEDGER_VERSION}\nevent={}\ntag={}\nbranch={}\nrecorded_at={}\nstatus={}\n",
            self.event.event.name(),
            self.event.tag.tag_name(),
            self.event.branch,
            self.recorded_at,
            if self.done { "done" } else { "pending" },
        );
        for digest in &self.ran {
            out.push_str(&format!("ran={digest}\n"));
        }
        out
    }

    fn parse(contents: &str) -> Result<Self, String> {
        let (mut version, mut event, mut tag, mut branch, mut recorded_at, mut done) = (None, None, None, None, None, None);
        let mut ran = Vec::new();
        for line in contents.lines() {
            let (key, value) = line.split_once('=').ok_or_else(|| format!("malformed line '{line}'"))?;
            match key {
                "version" => version = value.parse::<u32>().ok(),
                "event" => event = HookEvent::from_name(value),
                "tag" => tag = SemVer::parse(value),
                "branch" => branch = Some(value.to_string()),
                "recorded_at" => recorded_at = Some(value.to_string()),
                "status" => done = match value { "done" => Some(true), "pending" => Some(false), _ => None },
                "ran" => ran.push(value.to_string()),
                _ => {}
            }
        }
        if version != Some(LEDGER_VERSION) {
            return Err(format!("unsupported version (expected {LEDGER_VERSION})"));
        }
        Ok(Self {
            event: TagEvent {
                event: event.ok_or("missing or unknown event")?,
                tag: tag.ok_or("missing or invalid tag")?,
                branch: branch.ok_or("missing branch")?,
            },
            recorded_at: recorded_at.ok_or("missing recorded_at")?,
            done: done.ok_or("missing or unknown status")?,
            ran,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ledger() -> Ledger {
        Ledger {
            event: TagEvent::new(HookEvent::ReleaseFinish, &SemVer::new(1, 2, 0), "release/1.2.0"),
            recorded_at: "1700000000".to_string(),
            done: false,
            ran: vec!["abc".to_string(), "def".to_string()],
        }
    }

    #[test]
    fn a_ledger_survives_a_round_trip() {
        let mut l = ledger();
        assert_eq!(Ledger::parse(&l.serialize()).unwrap(), l);
        l.done = true;
        assert_eq!(Ledger::parse(&l.serialize()).unwrap(), l);
    }

    #[test]
    fn a_ledger_ignores_unknown_keys() {
        let l = ledger();
        assert_eq!(Ledger::parse(&format!("{}future=1\n", l.serialize())).unwrap(), l);
    }

    #[test]
    fn a_damaged_ledger_names_what_is_wrong() {
        let full = ledger().serialize();
        let without = |key: &str| full.lines().filter(|l| !l.starts_with(key)).collect::<Vec<_>>().join("\n");
        let cases = [
            (without("version="), "unsupported version (expected 1)"),
            (full.replace("version=1", "version=2"), "unsupported version (expected 1)"),
            (without("event="), "missing or unknown event"),
            (full.replace("event=release-finish", "event=release-bumpp"), "missing or unknown event"),
            (without("tag="), "missing or invalid tag"),
            (without("branch="), "missing branch"),
            (without("recorded_at="), "missing recorded_at"),
            (without("status="), "missing or unknown status"),
            (full.replace("status=pending", "status=maybe"), "missing or unknown status"),
            ("garbage".to_string(), "malformed line 'garbage'"),
        ];
        for (contents, expected) in cases {
            assert_eq!(Ledger::parse(&contents), Err(expected.to_string()), "contents: {contents}");
        }
    }

    #[test]
    fn a_missing_ledger_folder_has_nothing_pending() {
        let dir = crate::test_support::tmp_dir("gflow-hooks-ledger").join("absent");
        assert_eq!(Ledger::pending(&dir).unwrap(), Vec::new());
    }

    #[test]
    fn pending_ledgers_sort_by_age_then_name_and_skip_done_ones() {
        let dir = crate::test_support::tmp_dir("gflow-hooks-ledger");
        let at = |recorded_at: &str, tag: &str, done: bool| Ledger {
            event: TagEvent::new(HookEvent::ReleaseBump, &SemVer::parse(tag).unwrap(), "release/1.2.0"),
            recorded_at: recorded_at.to_string(),
            done,
            ran: Vec::new(),
        };
        for l in [at("20", "v1.2.0-rc.1", false), at("10", "v1.2.0-rc.3", false), at("10", "v1.2.0-rc.2", false), at("5", "v1.2.0-rc.4", true)] {
            l.save(&dir.join(Ledger::file_name(&l.event))).unwrap();
        }
        let order: Vec<String> = Ledger::pending(&dir).unwrap().into_iter().map(|(_, l)| l.event.tag.tag_name()).collect();
        assert_eq!(order, ["v1.2.0-rc.2", "v1.2.0-rc.3", "v1.2.0-rc.1"]);
        fs::remove_dir_all(&dir).ok();
    }
}

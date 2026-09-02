pub mod detect;
pub mod devops;
pub mod github;
pub mod template;

pub type Result<T> = std::result::Result<T, String>;

/// A pull request that has been merged: everything a finish needs to decide the
/// work is done and clean up. `head_sha` is the source-branch tip that was
/// merged — the only reliable "is my local branch the merged one" signal, since
/// squash/rebase merges break ancestor checks.
#[derive(Debug, Clone, PartialEq)]
pub struct MergedPr {
    pub url: String,
    pub head_sha: String,
    /// The commit the merge created on the base branch — its parent count
    /// reveals how the PR was completed (2 = merge commit, 1 = squash/rebase).
    pub merge_commit_sha: String,
    /// Short name of the branch the PR merged into (e.g. `develop`).
    pub base: String,
}

/// A PR that landed `head` into a specific `base`: everything a protected-mode
/// landing needs to confirm the merge and record its commit. Distinct from
/// `MergedPr` (which answers "is my work branch done, whatever the base") —
/// this answers "did head land into base", and carries the merge commit SHA
/// rather than the base name, since the base is already known by the caller.
#[derive(Debug, Clone, PartialEq)]
pub struct LandedPr {
    pub url: String,
    pub head_sha: String,
    pub merge_commit_sha: String,
}

/// What a new PR's description is made from. Flows own this policy; adapters
/// only execute it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PrBody<'a> {
    /// Body from this file (a resolved gflow template).
    File(&'a str),
    /// Fall back to the repository's own default PR template, else empty.
    NativeDefault,
    /// No description. Machinery PRs (landing legs) use this so the repo's
    /// native PR template never decorates an auto-generated merge.
    Empty,
}

pub trait HostingPlatform {
    /// Create a PR (or return the URL of an existing open one), with its
    /// description built per `body`.
    fn create_or_get_pr(&self, head: &str, base: &str, title: &str, body: PrBody<'_>) -> Result<String>;
    /// The merged PR for `head`, if the branch's most recent PR is merged.
    /// An open or abandoned newer PR yields `None` — the branch is still in play.
    fn merged_pr(&self, head: &str) -> Result<Option<MergedPr>>;
    /// The newest `head`→`base` PR, if it is merged. An open or abandoned newer
    /// PR yields `None`.
    fn merged_pr_to(&self, head: &str, base: &str) -> Result<Option<LandedPr>>;
    /// URL of the newest OPEN `head`→`base` PR, if any. Exists for the
    /// finish-branch migration: an open landing PR from an older gflow (head =
    /// the release/hotfix branch itself) must be surfaced, not duplicated.
    fn open_pr_to(&self, head: &str, base: &str) -> Result<Option<String>>;
    fn open_url(&self, url: &str) -> Result<()> {
        open_in_browser(url)
    }
    /// Put `text` on the user's clipboard. Default-implemented like `open_url`:
    /// a user-machine side effect, not provider knowledge.
    fn copy_text(&self, text: &str) -> Result<()> {
        copy_to_clipboard(text)
    }
    fn check_auth(&self) -> Result<()>;
}

/// Port for invoking a hosting CLI (`gh`, `az`, ...). The providers own the
/// policy — which flags to pass, which failures are normal — and depend on this
/// trait so that policy is testable without an installed CLI. `SystemCli` is the
/// only production implementation, keeping "no subprocess calls outside adapter
/// impls" (SKILL.md principle 1) true at a single point.
pub trait CliRunner {
    /// Run `program` with `args`, returning trimmed stdout on success or a
    /// `"<cli> <args> failed: <stderr>"` error.
    fn run(&self, program: &str, args: &[&str]) -> Result<String>;
}

/// The real runner: spawns the CLI as a child process.
pub struct SystemCli;

impl CliRunner for SystemCli {
    fn run(&self, program: &str, args: &[&str]) -> Result<String> {
        use std::process::Command;
        let output = Command::new(program).args(args).output()
            .map_err(|e| format!("Failed to run {program}: {e}"))?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            Err(format!("{program} {} failed: {stderr}", args.join(" ")))
        }
    }
}

/// PR-body precedence shared by all providers: a gflow-resolved template file
/// is used verbatim; `NativeDefault` probes the first existing native
/// default-template path, else `None` (empty body). The native path *lists*
/// stay per-provider knowledge on purpose.
fn resolve_body_file(body: PrBody<'_>, native_paths: &[&str]) -> Option<String> {
    match body {
        PrBody::File(p) => Some(p.to_string()),
        PrBody::NativeDefault => native_paths.iter()
            .find(|p| std::path::Path::new(p).exists())
            .map(|p| p.to_string()),
        PrBody::Empty => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{resolve_body_file, PrBody};

    #[test]
    fn gflow_template_wins_over_native_paths() {
        // The native path need not even exist for the template to win.
        let result = resolve_body_file(PrBody::File(".github/pr-templates/gflow-fix.md"), &["nope.md"]);
        assert_eq!(result, Some(".github/pr-templates/gflow-fix.md".to_string()));
    }

    #[test]
    fn no_template_and_no_existing_native_path_is_empty_body() {
        assert_eq!(resolve_body_file(PrBody::NativeDefault, &["definitely/not/a/real/path.md"]), None);
    }

    #[test]
    fn empty_body_ignores_an_existing_native_template() {
        // Machinery PRs (landing legs) must not inherit the repo's own PR
        // template — Cargo.toml stands in for a native path that exists.
        assert_eq!(resolve_body_file(PrBody::Empty, &["Cargo.toml"]), None);
    }
}

/// Write `text` to the OS clipboard by piping it into the platform's
/// clipboard tool (platform dispatch, provider-agnostic — a zero-policy shell
/// like `open_in_browser`). Callers treat failure as "no clipboard here".
#[cfg(target_os = "macos")]
const CLIPBOARD_TOOLS: &[&[&str]] = &[&["pbcopy"]];
#[cfg(target_os = "windows")]
const CLIPBOARD_TOOLS: &[&[&str]] = &[&["clip"]];
#[cfg(target_os = "linux")]
const CLIPBOARD_TOOLS: &[&[&str]] = &[&["wl-copy"], &["xclip", "-selection", "clipboard"], &["xsel", "-ib"]];
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
const CLIPBOARD_TOOLS: &[&[&str]] = &[];

pub fn copy_to_clipboard(text: &str) -> Result<()> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    CLIPBOARD_TOOLS
        .iter()
        .find_map(|cmd| {
            let mut child = Command::new(cmd[0]).args(&cmd[1..])
                .stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null())
                .spawn().ok()?;
            child.stdin.take()?.write_all(text.as_bytes()).ok()?;
            child.wait().ok()?.success().then_some(())
        })
        .ok_or_else(|| "no clipboard tool available".to_string())
}

/// Open a URL in the OS default browser (platform dispatch, provider-agnostic).
pub fn open_in_browser(url: &str) -> Result<()> {
    use std::process::Command;
    #[cfg(target_os = "macos")]
    let result = Command::new("open").arg(url).output();
    #[cfg(target_os = "windows")]
    let result = Command::new("cmd").args(["/C", "start", "", url]).output();
    #[cfg(target_os = "linux")]
    let result = Command::new("xdg-open").arg(url).output();
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    let result: std::io::Result<std::process::Output> =
        Err(std::io::Error::other("no browser-opener known for this platform"));

    let output = result.map_err(|e| format!("Failed to open URL: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(format!("Failed to open URL: {stderr}"))
    }
}

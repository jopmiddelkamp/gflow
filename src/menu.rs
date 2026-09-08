use std::io::{self, Write};
use crossterm::{
    cursor, event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute, queue,
    style::{self, Stylize},
    terminal,
};
use crate::action::{validate_branch_name, Action};
use crate::git::branch::BranchType;
use crate::prompt::Prompter;

/// The real `Prompter`: the interactive select menu on stderr.
pub struct MenuPrompter;

impl Prompter for MenuPrompter {
    fn select(&self, prompt: &str, items: &[&str]) -> Result<usize, String> {
        show_select(prompt, items)
    }

    fn prompt_name(&self, prompt: &str) -> Result<String, String> {
        prompt_name(prompt)
    }

    fn prompt_line(&self, prompt: &str) -> Result<String, String> {
        prompt_line(prompt)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ReleaseOption {
    FinishRelease, StartReleaseFix, BumpVersion, SyncWithDevelop,
}

impl ReleaseOption {
    pub fn label(&self) -> &'static str {
        match self {
            Self::FinishRelease => "finish release",
            Self::StartReleaseFix => "start release fix",
            Self::BumpVersion => "bump version",
            Self::SyncWithDevelop => "sync with develop",
        }
    }

    const ALL: [Self; 4] = [Self::FinishRelease, Self::StartReleaseFix, Self::BumpVersion, Self::SyncWithDevelop];
}

/// Enables raw mode on construction; restores the terminal (cursor visible, raw
/// mode off) on drop. Every exit path — success, error, Ctrl-C/Esc abort — runs
/// the same cleanup structurally, so the documented "no raw-mode leak" invariant
/// is enforced by the type, not by hand-written cleanup at each return.
struct TerminalGuard;

impl TerminalGuard {
    fn enter(context: &str) -> Result<Self, String> {
        terminal::enable_raw_mode().map_err(|e| format!("{context}: {e}"))?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(io::stderr(), cursor::Show);
        let _ = terminal::disable_raw_mode();
    }
}

fn render_menu(out: &mut io::Stderr, items: &[&str], selected: usize) -> io::Result<()> {
    for (i, item) in items.iter().enumerate() {
        let number = i + 1;
        queue!(out, cursor::MoveToColumn(0), terminal::Clear(terminal::ClearType::CurrentLine))?;
        if i == selected {
            queue!(
                out,
                style::PrintStyledContent(format!("> {number}) {item}").cyan().bold()),
            )?;
        } else {
            queue!(
                out,
                style::PrintStyledContent(format!("  {number}) {item}").dim()),
            )?;
        }
        if i < items.len() - 1 {
            queue!(out, style::Print("\r\n"))?;
        }
    }
    out.flush()?;
    Ok(())
}

pub fn show_select(prompt: &str, items: &[&str]) -> Result<usize, String> {
    if items.is_empty() {
        return Err("Menu error: no items to select from".to_string());
    }

    let mut out = io::stderr();
    let mut selected: usize = 0;

    // Print prompt (clear line first to avoid ghost text from previous prompts)
    execute!(
        out,
        cursor::MoveToColumn(0),
        terminal::Clear(terminal::ClearType::CurrentLine),
        style::PrintStyledContent("? ".green().bold()),
        style::Print(prompt),
        style::Print("\n"),
    ).map_err(|e| format!("Menu error: {e}"))?;

    let _guard = TerminalGuard::enter("Menu error")?;

    // Hide cursor during selection; the guard re-shows it on every exit path.
    execute!(out, cursor::Hide).map_err(|e| format!("Menu error: {e}"))?;

    // Initial render
    render_menu(&mut out, items, selected).map_err(|e| format!("Menu error: {e}"))?;

    let result = loop {
        let ev = event::read().map_err(|e| format!("Menu error: {e}"))?;

        // On Windows, crossterm emits Press + Release events; only handle Press
        let Event::Key(KeyEvent { kind: KeyEventKind::Press, code, modifiers, .. }) = ev else {
            continue;
        };

        match (code, modifiers) {
            (KeyCode::Char('c'), KeyModifiers::CONTROL) | (KeyCode::Esc, _) => {
                return Err("Aborted".to_string());
            }
            (KeyCode::Enter, _) => {
                break selected;
            }
            (KeyCode::Up, _) => {
                if selected > 0 {
                    selected -= 1;
                }
            }
            (KeyCode::Down, _) => {
                if selected < items.len() - 1 {
                    selected += 1;
                }
            }
            (KeyCode::Char(c), KeyModifiers::NONE) => {
                if let Some(digit) = c.to_digit(10) {
                    let idx = digit as usize;
                    if idx >= 1 && idx <= items.len() && idx <= 9 {
                        selected = idx - 1;
                        // Re-render to show selection highlighted before returning
                        if items.len() > 1 {
                            let _ = execute!(out, cursor::MoveUp((items.len() - 1) as u16));
                        }
                        let _ = execute!(out, cursor::MoveToColumn(0));
                        let _ = render_menu(&mut out, items, selected);
                        break selected;
                    }
                }
            }
            _ => {}
        }

        // Redraw: move cursor up to start of menu, then re-render
        if items.len() > 1 {
            execute!(out, cursor::MoveUp((items.len() - 1) as u16))
                .map_err(|e| format!("Menu error: {e}"))?;
        }
        execute!(out, cursor::MoveToColumn(0)).map_err(|e| format!("Menu error: {e}"))?;
        render_menu(&mut out, items, selected).map_err(|e| format!("Menu error: {e}"))?;
    };

    // Move past the menu; the guard restores cursor + raw mode on drop.
    let _ = execute!(out, style::Print("\r\n"));

    Ok(result)
}

/// Print `prompt` and read a line of input in raw mode. Shared scaffolding for
/// `prompt_name`/`prompt_line`: prompt printing, raw-mode lifecycle, the
/// Windows Press-filter, Ctrl-C/Esc abort, Enter, and backspace handling all
/// live here. `transform` decides which char (if any) each keystroke appends,
/// given the buffer typed so far.
fn read_raw_line(prompt: &str, transform: impl Fn(&str, char) -> Option<char>) -> Result<String, String> {
    let mut out = io::stderr();
    let mut input = String::new();

    execute!(
        out,
        style::PrintStyledContent("? ".green().bold()),
        style::Print(format!("{prompt}: ")),
    ).map_err(|e| format!("Input error: {e}"))?;

    let _guard = TerminalGuard::enter("Input error")?;

    let result = loop {
        let ev = event::read().map_err(|e| format!("Input error: {e}"))?;

        // On Windows, crossterm emits Press + Release events; only handle Press
        let Event::Key(KeyEvent { kind: KeyEventKind::Press, code, modifiers, .. }) = ev else {
            continue;
        };

        match (code, modifiers) {
            (KeyCode::Char('c'), KeyModifiers::CONTROL) | (KeyCode::Esc, _) => {
                return Err("Aborted".to_string());
            }
            (KeyCode::Enter, _) => break input,
            (KeyCode::Backspace, _) => {
                if input.pop().is_some() {
                    let _ = execute!(out, cursor::MoveLeft(1), style::Print(" "), cursor::MoveLeft(1));
                }
            }
            (KeyCode::Char(c), _) => {
                if let Some(ch) = transform(&input, c) {
                    input.push(ch);
                    let _ = execute!(out, style::Print(ch));
                }
            }
            _ => {}
        }
    };

    // A real newline, not a cursor move: on the terminal's last row a cursor
    // move cannot scroll, so the next output would overwrite this prompt.
    let _ = execute!(out, style::Print("\r\n"));
    Ok(result)
}

/// Shape one branch-name keystroke, given the buffer typed so far: spaces
/// become hyphens and consecutive hyphens collapse (`None` = swallow the key).
/// Input shaping over validation — an invalid name is untypeable rather than
/// rejected after the fact (decisions.md, CLI/UX Conventions).
fn shape_branch_name_char(typed: &str, c: char) -> Option<char> {
    let ch = if c == ' ' { '-' } else { c };
    if ch == '-' && typed.ends_with('-') { None } else { Some(ch) }
}

pub fn prompt_name(prompt: &str) -> Result<String, String> {
    loop {
        let result = read_raw_line(prompt, shape_branch_name_char)?;

        // Trim leading/trailing hyphens
        let trimmed = result.trim_matches('-').to_string();

        match validate_branch_name(&trimmed) {
            Ok(()) => return Ok(trimmed),
            Err(e) => {
                let _ = execute!(
                    io::stderr(),
                    style::PrintStyledContent(format!("  {e}").red()),
                    style::Print("\r\n"),
                );
                // Loop to re-prompt
            }
        }
    }
}

/// Prompt for a free-form line of text (spaces, slashes, `~` all allowed, no
/// validation). Unlike `prompt_name`, this does not mangle input into a branch
/// name — use it for paths and shell commands.
pub fn prompt_line(prompt: &str) -> Result<String, String> {
    Ok(read_raw_line(prompt, |_, c| Some(c))?.trim().to_string())
}

pub fn show_menu(prompter: &dyn Prompter, branch_type: &BranchType, current_branch: &str, main_branch: &str) -> Result<Action, String> {
    match branch_type {
        BranchType::Main => {
            let labels = &["start hotfix fix"];
            prompter.select("What would you like to do?", labels)?;
            let name = prompter.prompt_name("Name for hotfix-fix branch")?;
            Ok(Action::StartHotfixFix { name, no_checkout: false, no_worktree: false })
        }
        BranchType::Develop => {
            // "start <kind>" for every work-branch kind, then "start release".
            let kinds = BranchType::work_kinds();
            let mut labels: Vec<String> = kinds.iter().map(|k| format!("start {k}")).collect();
            labels.push("start release".to_string());
            let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
            let idx = prompter.select("What would you like to do?", &label_refs)?;
            match kinds.get(idx) {
                Some(kind) => {
                    let name = prompter.prompt_name(&format!("Name for {kind} branch"))?;
                    Ok(Action::StartWorkBranch { prefix: kind.to_string(), name, from: "develop".to_string(), no_checkout: false, no_worktree: false })
                }
                None => Ok(Action::StartRelease { release_type: None, no_worktree: false }),
            }
        }
        BranchType::Feature { .. } | BranchType::Fix { .. } | BranchType::Chore { .. }
        | BranchType::Docs { .. } | BranchType::Refactor { .. } => {
            let current_kind = branch_type.work_kind()
                .expect("this match arm only accepts work branches");
            // "finish <current kind>", then "start <kind>" for every kind.
            let kinds = BranchType::work_kinds();
            let mut labels: Vec<String> = vec![format!("finish {current_kind}")];
            labels.extend(kinds.iter().map(|k| format!("start {k}")));
            let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
            let idx = prompter.select("What would you like to do?", &label_refs)?;
            if idx == 0 {
                return Ok(Action::FinishWorkBranch { breaking: None, base: None });
            }
            let kind = kinds[idx - 1];
            let name = prompter.prompt_name(&format!("Name for {kind} branch"))?;
            let current_label = format!("{current_branch} (current)");
            let base_options: &[&str] = &[&current_label, "develop"];
            let base_idx = prompter.select("Base branch", base_options)?;
            let from = if base_idx == 0 { current_branch.to_string() } else { "develop".to_string() };
            Ok(Action::StartWorkBranch { prefix: kind.to_string(), name, from, no_checkout: false, no_worktree: false })
        }
        BranchType::ReleaseFix { .. } => {
            prompter.select("What would you like to do?", &["finish release fix"])?;
            Ok(Action::FinishReleaseFix)
        }
        BranchType::ReleaseChore { .. } => {
            prompter.select("What would you like to do?", &["finish release chore"])?;
            Ok(Action::FinishReleaseChore)
        }
        BranchType::Release { .. } => {
            let labels: Vec<&str> = ReleaseOption::ALL.iter().map(|o| o.label()).collect();
            let idx = prompter.select("What would you like to do?", &labels)?;
            match ReleaseOption::ALL[idx] {
                ReleaseOption::StartReleaseFix => {
                    let name = prompter.prompt_name("Name for release-fix branch")?;
                    Ok(Action::StartReleaseFix { name, no_checkout: false, no_worktree: false })
                }
                ReleaseOption::BumpVersion => Ok(Action::BumpVersion),
                ReleaseOption::SyncWithDevelop => Ok(Action::SyncWithDevelop),
                ReleaseOption::FinishRelease => Ok(Action::FinishRelease),
            }
        }
        BranchType::HotfixFix { .. } => {
            prompter.select("What would you like to do?", &["finish hotfix fix"])?;
            Ok(Action::FinishHotfixFix)
        }
        BranchType::Hotfix { .. } => {
            let idx = prompter.select("What would you like to do?", &["finish hotfix", "start hotfix fix"])?;
            if idx == 0 {
                return Ok(Action::FinishHotfix);
            }
            let name = prompter.prompt_name("Name for hotfix-fix branch")?;
            Ok(Action::StartHotfixFix { name, no_checkout: false, no_worktree: false })
        }
        BranchType::Other => Err(format!("Not on a recognized gitflow branch. Switch to {main_branch} or develop first.")),
    }
}

#[cfg(test)]
mod tests {
    use super::shape_branch_name_char;

    /// Type `keys` one at a time through the shaper, as the prompt does.
    fn typed(keys: &str) -> String {
        let mut buf = String::new();
        for c in keys.chars() {
            if let Some(ch) = shape_branch_name_char(&buf, c) {
                buf.push(ch);
            }
        }
        buf
    }

    #[test]
    fn spaces_become_hyphens_as_you_type() {
        assert_eq!(typed("passkey login"), "passkey-login");
    }

    #[test]
    fn consecutive_hyphens_collapse_however_they_were_typed() {
        assert_eq!(typed("passkey  login"), "passkey-login");
        assert_eq!(typed("passkey--login"), "passkey-login");
        assert_eq!(typed("passkey - login"), "passkey-login");
    }

    #[test]
    fn ordinary_characters_pass_through_untouched() {
        assert_eq!(typed("fix_bug.2"), "fix_bug.2");
    }

    #[test]
    fn a_leading_hyphen_is_still_typeable_and_trimmed_later() {
        // The shaper only collapses; prompt_name trims the ends afterwards.
        assert_eq!(typed("-login"), "-login");
    }
}

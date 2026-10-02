use std::io::{self, Write};
use crossterm::{
    cursor, event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    style::{self, Stylize},
    terminal,
};
use crate::action::{validate_branch_name, Action};
use crate::git::branch::BranchType;
use crate::prompt::Prompter;

macro_rules! queue_terminal {
    ($out:expr, $($command:expr),+ $(,)?) => {{
        (|| -> io::Result<()> {
            $($out.queue_command($command)?;)+
            Ok(())
        })()
    }};
}

macro_rules! execute_terminal {
    ($out:expr, $($command:expr),+ $(,)?) => {{
        queue_terminal!($out, $($command),+).and_then(|()| $out.flush())
    }};
}

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

trait Terminal: Write {
    fn queue_command(&mut self, command: impl crossterm::Command) -> io::Result<()>;
    fn queue_styled(&mut self, text: style::StyledContent<impl std::fmt::Display>) -> io::Result<()>;
    fn enable_raw_mode(&mut self) -> io::Result<()>;
    fn disable_raw_mode(&mut self) -> io::Result<()>;
    fn read_event(&mut self) -> io::Result<Event>;
}

struct SystemTerminal(io::Stderr);

impl Write for SystemTerminal {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl Terminal for SystemTerminal {
    fn queue_command(&mut self, command: impl crossterm::Command) -> io::Result<()> {
        crossterm::queue!(self, command)
    }

    fn queue_styled(&mut self, text: style::StyledContent<impl std::fmt::Display>) -> io::Result<()> {
        crossterm::queue!(self, style::PrintStyledContent(text))
    }

    fn enable_raw_mode(&mut self) -> io::Result<()> {
        terminal::enable_raw_mode()
    }

    fn disable_raw_mode(&mut self) -> io::Result<()> {
        terminal::disable_raw_mode()
    }

    fn read_event(&mut self) -> io::Result<Event> {
        event::read()
    }
}

/// Restores cursor visibility and raw mode on every exit path.
struct TerminalGuard<'a, T: Terminal>(&'a mut T);

impl<'a, T: Terminal> TerminalGuard<'a, T> {
    fn enter(out: &'a mut T, context: &str) -> Result<Self, String> {
        out.enable_raw_mode().map_err(|e| format!("{context}: {e}"))?;
        Ok(Self(out))
    }
}

impl<T: Terminal> Drop for TerminalGuard<'_, T> {
    fn drop(&mut self) {
        let _ = execute_terminal!(self.0, cursor::Show);
        let _ = self.0.disable_raw_mode();
    }
}

fn render_menu(out: &mut impl Terminal, items: &[&str], selected: usize) -> io::Result<()> {
    for (i, item) in items.iter().enumerate() {
        let number = i + 1;
        queue_terminal!(out, cursor::MoveToColumn(0), terminal::Clear(terminal::ClearType::CurrentLine))?;
        if i == selected {
            out.queue_styled(format!("> {number}) {item}").cyan().bold())?;
        } else {
            out.queue_styled(format!("  {number}) {item}").dim())?;
        }
        if i < items.len() - 1 {
            queue_terminal!(out, style::Print("\r\n"))?;
        }
    }
    out.flush()?;
    Ok(())
}

pub fn show_select(prompt: &str, items: &[&str]) -> Result<usize, String> {
    show_select_on(&mut SystemTerminal(io::stderr()), prompt, items)
}

fn show_select_on(out: &mut impl Terminal, prompt: &str, items: &[&str]) -> Result<usize, String> {
    if items.is_empty() {
        return Err("Menu error: no items to select from".to_string());
    }

    let mut selected: usize = 0;

    // Print prompt (clear line first to avoid ghost text from previous prompts)
    (|| {
        queue_terminal!(out, cursor::MoveToColumn(0), terminal::Clear(terminal::ClearType::CurrentLine))?;
        out.queue_styled("? ".green().bold())?;
        execute_terminal!(out, style::Print(prompt), style::Print("\n"))
    })().map_err(|e| format!("Menu error: {e}"))?;

    let guard = TerminalGuard::enter(out, "Menu error")?;
    let out = &mut *guard.0;

    // Hide cursor during selection; the guard re-shows it on every exit path.
    execute_terminal!(out, cursor::Hide).map_err(|e| format!("Menu error: {e}"))?;

    // Initial render
    render_menu(out, items, selected).map_err(|e| format!("Menu error: {e}"))?;

    let result = loop {
        let ev = out.read_event().map_err(|e| format!("Menu error: {e}"))?;

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
                            let _ = execute_terminal!(out, cursor::MoveUp((items.len() - 1) as u16));
                        }
                        let _ = execute_terminal!(out, cursor::MoveToColumn(0));
                        let _ = render_menu(out, items, selected);
                        break selected;
                    }
                }
            }
            _ => {}
        }

        // Redraw: move cursor up to start of menu, then re-render
        if items.len() > 1 {
            execute_terminal!(out, cursor::MoveUp((items.len() - 1) as u16))
                .map_err(|e| format!("Menu error: {e}"))?;
        }
        execute_terminal!(out, cursor::MoveToColumn(0)).map_err(|e| format!("Menu error: {e}"))?;
        render_menu(out, items, selected).map_err(|e| format!("Menu error: {e}"))?;
    };

    // Move past the menu; the guard restores cursor + raw mode on drop.
    let _ = execute_terminal!(out, style::Print("\r\n"));

    Ok(result)
}

/// Print `prompt` and read a line of input in raw mode. Shared scaffolding for
/// `prompt_name`/`prompt_line`: prompt printing, raw-mode lifecycle, the
/// Windows Press-filter, Ctrl-C/Esc abort, Enter, and backspace handling all
/// live here. `transform` decides which char (if any) each keystroke appends,
/// given the buffer typed so far.
fn read_raw_line(out: &mut impl Terminal, prompt: &str, transform: impl Fn(&str, char) -> Option<char>) -> Result<String, String> {
    let mut input = String::new();

    out.queue_styled("? ".green().bold())
        .and_then(|()| execute_terminal!(out, style::Print(format!("{prompt}: "))))
        .map_err(|e| format!("Input error: {e}"))?;

    let guard = TerminalGuard::enter(out, "Input error")?;
    let out = &mut *guard.0;

    let result = loop {
        let ev = out.read_event().map_err(|e| format!("Input error: {e}"))?;

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
                    let _ = execute_terminal!(out, cursor::MoveLeft(1), style::Print(" "), cursor::MoveLeft(1));
                }
            }
            (KeyCode::Char(c), _) => {
                if let Some(ch) = transform(&input, c) {
                    input.push(ch);
                    let _ = execute_terminal!(out, style::Print(ch));
                }
            }
            _ => {}
        }
    };

    // A real newline, not a cursor move: on the terminal's last row a cursor
    // move cannot scroll, so the next output would overwrite this prompt.
    let _ = execute_terminal!(out, style::Print("\r\n"));
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
    prompt_name_on(&mut SystemTerminal(io::stderr()), prompt)
}

fn prompt_name_on(out: &mut impl Terminal, prompt: &str) -> Result<String, String> {
    loop {
        let result = read_raw_line(out, prompt, shape_branch_name_char)?;

        // Trim leading/trailing hyphens
        let trimmed = result.trim_matches('-').to_string();

        match validate_branch_name(&trimmed) {
            Ok(()) => return Ok(trimmed),
            Err(e) => {
                let _ = out.queue_styled(format!("  {e}").red())
                    .and_then(|()| execute_terminal!(out, style::Print("\r\n")));
                // Loop to re-prompt
            }
        }
    }
}

/// Prompt for a free-form line of text (spaces, slashes, `~` all allowed, no
/// validation). Unlike `prompt_name`, this does not mangle input into a branch
/// name — use it for paths and shell commands.
pub fn prompt_line(prompt: &str) -> Result<String, String> {
    prompt_line_on(&mut SystemTerminal(io::stderr()), prompt)
}

fn prompt_line_on(out: &mut impl Terminal, prompt: &str) -> Result<String, String> {
    Ok(read_raw_line(out, prompt, |_, c| Some(c))?.trim().to_string())
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
    use super::*;
    use std::collections::VecDeque;

    struct ScriptedTerminal {
        events: VecDeque<io::Result<Event>>,
        output: Vec<u8>,
        raw: bool,
        entered: usize,
        exited: usize,
        fail_enter: bool,
        fail_write_at: Option<usize>,
        fail_flush_at: Option<usize>,
        writes: usize,
        flushes: usize,
        output_at_read: Vec<(usize, usize)>,
        styled: Vec<(String, style::ContentStyle)>,
    }

    impl ScriptedTerminal {
        fn new(events: impl IntoIterator<Item = Event>) -> Self {
            Self {
                events: events.into_iter().map(Ok).collect(),
                output: Vec::new(),
                raw: false,
                entered: 0,
                exited: 0,
                fail_enter: false,
                fail_write_at: None,
                fail_flush_at: None,
                writes: 0,
                flushes: 0,
                output_at_read: Vec::new(),
                styled: Vec::new(),
            }
        }

        fn text(&self) -> String {
            String::from_utf8(self.output.clone()).unwrap()
        }

        fn assert_restored(&self) {
            assert!(!self.raw, "terminal must leave raw mode");
            assert_eq!(self.exited, self.entered);
            if self.entered > 0 {
                assert!(self.text().contains("\x1b[?25h"), "cursor must be shown");
            }
        }
    }

    impl Write for ScriptedTerminal {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.writes += 1;
            if self.fail_write_at == Some(self.writes) {
                return Err(io::Error::other("output unavailable"));
            }
            self.output.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flushes += 1;
            if self.fail_flush_at == Some(self.flushes) {
                return Err(io::Error::other("flush unavailable"));
            }
            Ok(())
        }
    }

    impl Terminal for ScriptedTerminal {
        fn queue_command(&mut self, command: impl crossterm::Command) -> io::Result<()> {
            let mut encoded = String::new();
            command.write_ansi(&mut encoded).unwrap();
            self.write_all(encoded.as_bytes())
        }

        fn queue_styled(&mut self, text: style::StyledContent<impl std::fmt::Display>) -> io::Result<()> {
            let value = text.content().to_string();
            self.styled.push((value.clone(), *text.style()));
            self.write_all(value.as_bytes())
        }

        fn enable_raw_mode(&mut self) -> io::Result<()> {
            if self.fail_enter {
                return Err(io::Error::other("raw mode unavailable"));
            }
            self.raw = true;
            self.entered += 1;
            Ok(())
        }

        fn disable_raw_mode(&mut self) -> io::Result<()> {
            self.raw = false;
            self.exited += 1;
            Ok(())
        }

        fn read_event(&mut self) -> io::Result<Event> {
            assert!(self.raw, "events must be read in raw mode");
            self.output_at_read.push((self.writes, self.flushes));
            self.events.pop_front().expect("unexpected request for input")
        }
    }

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn keys(text: &str) -> Vec<Event> {
        text.chars().map(|c| key(KeyCode::Char(c))).chain([key(KeyCode::Enter)]).collect()
    }

    #[test]
    fn arrow_selection_is_bounded_and_restores_the_terminal() {
        let mut terminal = ScriptedTerminal::new([
            key(KeyCode::Up), key(KeyCode::Down), key(KeyCode::Down),
            key(KeyCode::Down), key(KeyCode::Up), key(KeyCode::Enter),
        ]);

        assert_eq!(show_select_on(&mut terminal, "Pick", &["first", "second", "third"]), Ok(1));
        assert!(terminal.text().contains("> 2) second"));
        assert!(terminal.text().ends_with("\r\n\x1b[?25h"));
        terminal.assert_restored();
    }

    #[test]
    fn digit_selection_confirms_only_available_one_to_nine_entries() {
        for (items, input, selected) in [
            (vec!["only"], "0x92", 0),
            (vec!["first", "second"], "03x2", 1),
            (vec!["1", "2", "3", "4", "5", "6", "7", "8", "9", "10"], "09", 8),
        ] {
            let mut terminal = ScriptedTerminal::new(keys(input));
            assert_eq!(show_select_on(&mut terminal, "Pick", &items), Ok(selected));
            terminal.assert_restored();
        }
        let mut terminal = ScriptedTerminal::new([key(KeyCode::Char('1'))]);
        assert_eq!(show_select_on(&mut terminal, "Pick", &["only"]), Ok(0));
        terminal.assert_restored();
    }

    #[test]
    fn selection_ignores_key_releases_and_non_key_events() {
        let mut terminal = ScriptedTerminal::new([
            Event::Resize(80, 24),
            Event::Key(KeyEvent::new_with_kind(KeyCode::Down, KeyModifiers::NONE, KeyEventKind::Release)),
            Event::Key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::CONTROL)),
            key(KeyCode::Left), key(KeyCode::Enter),
        ]);
        assert_eq!(show_select_on(&mut terminal, "Pick", &["first", "second"]), Ok(0));
        terminal.assert_restored();
    }

    #[test]
    fn empty_menus_fail_before_opening_the_terminal() {
        let mut terminal = ScriptedTerminal::new([]);
        assert_eq!(show_select_on(&mut terminal, "Pick", &[]), Err("Menu error: no items to select from".into()));
        assert!(terminal.output.is_empty());
        assert_eq!(terminal.entered, 0);
    }

    #[test]
    fn free_form_input_preserves_characters_and_supports_backspace() {
        let mut events = vec![key(KeyCode::Backspace), Event::FocusGained,
            Event::Key(KeyEvent::new_with_kind(KeyCode::Char('x'), KeyModifiers::NONE, KeyEventKind::Release)),
            key(KeyCode::Left)];
        events.extend("  ~/my folder/x".chars().map(|c| key(KeyCode::Char(c))));
        events.extend([key(KeyCode::Backspace), key(KeyCode::Char('é')), key(KeyCode::Backspace), key(KeyCode::Char('z')), key(KeyCode::Enter)]);
        let mut terminal = ScriptedTerminal::new(events);
        assert_eq!(prompt_line_on(&mut terminal, "Path"), Ok("~/my folder/z".into()));
        assert!(terminal.text().contains("Path: "));
        assert!(terminal.text().contains("\x1b[1D \x1b[1D"));
        terminal.assert_restored();
    }

    #[test]
    fn branch_names_are_shaped_and_invalid_names_are_reprompted() {
        let mut events = vec![
            key(KeyCode::Left), Event::FocusGained,
            Event::Key(KeyEvent::new_with_kind(KeyCode::Char('x'), KeyModifiers::NONE, KeyEventKind::Release)),
            key(KeyCode::Backspace), key(KeyCode::Char('x')), key(KeyCode::Backspace),
        ];
        events.extend(keys("---"));
        events.extend(keys("bad..name"));
        events.extend(keys(" --passkey  login-- "));
        let mut terminal = ScriptedTerminal::new(events);

        assert_eq!(prompt_name_on(&mut terminal, "Name"), Ok("passkey-login".into()));
        assert_eq!(terminal.entered, 3);
        assert!(terminal.text().contains("Name cannot be empty"));
        assert!(terminal.text().contains("Invalid branch name"));
        terminal.assert_restored();
    }

    #[test]
    fn escape_and_control_c_abort_selection_and_both_text_prompts() {
        for event in [key(KeyCode::Esc), Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))] {
            let mut terminal = ScriptedTerminal::new([event.clone()]);
            assert_eq!(show_select_on(&mut terminal, "Pick", &["only"]), Err("Aborted".into()));
            terminal.assert_restored();
            let mut terminal = ScriptedTerminal::new([event.clone()]);
            assert_eq!(prompt_name_on(&mut terminal, "Name"), Err("Aborted".into()));
            terminal.assert_restored();
            let mut terminal = ScriptedTerminal::new([event]);
            assert_eq!(prompt_line_on(&mut terminal, "Line"), Err("Aborted".into()));
            terminal.assert_restored();
        }
    }

    #[test]
    fn raw_mode_failures_keep_input_unread_and_name_the_operation() {
        let mut terminal = ScriptedTerminal::new([]);
        terminal.fail_enter = true;
        assert_eq!(show_select_on(&mut terminal, "Pick", &["only"]), Err("Menu error: raw mode unavailable".into()));
        assert_eq!(prompt_line_on(&mut terminal, "Line"), Err("Input error: raw mode unavailable".into()));
        assert_eq!(prompt_name_on(&mut terminal, "Name"), Err("Input error: raw mode unavailable".into()));
        terminal.assert_restored();
    }

    #[test]
    fn event_read_failures_restore_the_terminal_and_name_the_operation() {
        let mut terminal = ScriptedTerminal::new([]);
        terminal.events.push_back(Err(io::Error::other("input unavailable")));
        assert_eq!(show_select_on(&mut terminal, "Pick", &["only"]), Err("Menu error: input unavailable".into()));
        terminal.assert_restored();
        let mut terminal = ScriptedTerminal::new([]);
        terminal.events.push_back(Err(io::Error::other("input unavailable")));
        assert_eq!(prompt_line_on(&mut terminal, "Line"), Err("Input error: input unavailable".into()));
        terminal.assert_restored();
        let mut terminal = ScriptedTerminal::new([]);
        terminal.events.push_back(Err(io::Error::other("input unavailable")));
        assert_eq!(prompt_name_on(&mut terminal, "Name"), Err("Input error: input unavailable".into()));
        terminal.assert_restored();
    }

    #[test]
    fn menu_rendering_marks_only_the_selected_entry() {
        let mut terminal = ScriptedTerminal::new([]);
        render_menu(&mut terminal, &["first", "second"], 1).unwrap();
        assert_eq!(terminal.styled, vec![
            ("  1) first".into(), *"".dim().style()),
            ("> 2) second".into(), *"".cyan().bold().style()),
        ]);
    }

    #[test]
    fn menu_output_failures_stop_selection_and_always_restore_raw_mode() {
        let events = [key(KeyCode::Down), key(KeyCode::Enter)];
        let mut successful = ScriptedTerminal::new(events.clone());
        assert_eq!(show_select_on(&mut successful, "Pick", &["first", "second"]), Ok(1));
        let (required_writes, required_flushes) = successful.output_at_read[1];
        for index in 1..=successful.writes {
            let mut terminal = ScriptedTerminal::new(events.clone());
            terminal.fail_write_at = Some(index);
            let result = show_select_on(&mut terminal, "Pick", &["first", "second"]);
            let expected = if index <= required_writes { Err("Menu error: output unavailable".into()) } else { Ok(1) };
            assert_eq!(result, expected, "write {index}");
            assert!(!terminal.raw);
            assert_eq!(terminal.exited, terminal.entered);
        }
        for index in 1..=successful.flushes {
            let mut terminal = ScriptedTerminal::new(events.clone());
            terminal.fail_flush_at = Some(index);
            let result = show_select_on(&mut terminal, "Pick", &["first", "second"]);
            let expected = if index <= required_flushes { Err("Menu error: flush unavailable".into()) } else { Ok(1) };
            assert_eq!(result, expected, "flush {index}");
            assert!(!terminal.raw);
            assert_eq!(terminal.exited, terminal.entered);
        }
    }

    #[test]
    fn input_prompt_output_failures_stop_before_reading_a_value() {
        let mut terminal = ScriptedTerminal::new([]);
        terminal.fail_write_at = Some(1);
        assert_eq!(prompt_line_on(&mut terminal, "Line"), Err("Input error: output unavailable".into()));
        assert_eq!(terminal.entered, 0);
        let mut terminal = ScriptedTerminal::new([]);
        terminal.fail_flush_at = Some(1);
        assert_eq!(prompt_name_on(&mut terminal, "Name"), Err("Input error: flush unavailable".into()));
        assert_eq!(terminal.entered, 0);
    }

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

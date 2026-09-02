//! Setup commands run inside a freshly created worktree, read from the same
//! files Cursor and worktree-cli use: `.cursor/worktrees.json` (a JSON array)
//! or `worktrees.json` (`{"setup-worktree": [...]}`) at the repo root.

use std::path::{Path, PathBuf};

pub const CURSOR_FILE: &str = ".cursor/worktrees.json";
pub const GENERIC_FILE: &str = "worktrees.json";

#[derive(Debug, Clone, PartialEq)]
pub struct SetupCommands {
    pub file: PathBuf,
    pub commands: Vec<String>,
}

/// Extract the setup commands: a top-level array of strings, or an object whose
/// `setup-worktree` key holds one (other keys are skipped whatever they hold).
/// A hand-rolled subset on purpose — two fixed shapes do not justify a JSON crate.
pub fn parse(json: &str) -> Result<Vec<String>, String> {
    let mut c = Cursor { bytes: json.as_bytes(), pos: 0 };
    c.skip_ws();
    let commands = match c.peek() {
        Some(b'[') => c.string_array("setup commands")?,
        Some(b'{') => c.object_setup_key()?,
        _ => return Err(c.error("expected a JSON array or object")),
    };
    c.skip_ws();
    if c.pos != c.bytes.len() {
        return Err(c.error("unexpected trailing content"));
    }
    Ok(commands)
}

/// Find the setup file for this working tree: Cursor's first, then the generic
/// one; a present-but-empty file falls through to the next. `None` = feature off.
pub fn load(worktree_root: &Path) -> Result<Option<SetupCommands>, String> {
    for name in [CURSOR_FILE, GENERIC_FILE] {
        let file = worktree_root.join(name);
        if !file.exists() {
            continue;
        }
        let contents = std::fs::read_to_string(&file)
            .map_err(|e| format!("Failed to read {}: {e}", file.display()))?;
        let commands = parse(&contents).map_err(|e| format!("{}: {e}", file.display()))?;
        if !commands.is_empty() {
            return Ok(Some(SetupCommands { file, commands }));
        }
    }
    Ok(None)
}

/// The setup commands this run should use, plus a warning to print if the repo's
/// file could not be used. A file gflow cannot parse is reported and ignored
/// rather than fatal: setup commands never fail a start, so they must not fail
/// every other command either — and with the worktree flow off the file is not
/// read at all, because nothing in the run could act on it.
pub fn resolve(worktree_root: &Path, worktree_enabled: bool) -> (Option<SetupCommands>, Option<String>) {
    if !worktree_enabled {
        return (None, None);
    }
    match load(worktree_root) {
        Ok(commands) => (commands, None),
        Err(e) => (None, Some(format!("{e}\n  Setup commands skipped."))),
    }
}

/// Port: run one setup command inside a freshly created worktree.
pub trait WorktreeSetup {
    fn run_command(&self, worktree: &Path, main_root: &Path, command: &str) -> Result<(), String>;
}

fn shell_argv(command: &str, windows: bool) -> (&'static str, Vec<String>) {
    if windows {
        ("cmd", vec!["/C".to_string(), command.to_string()])
    } else {
        ("sh", vec!["-c".to_string(), command.to_string()])
    }
}

/// Production `WorktreeSetup`: a zero-policy shell like `CommandEditor::open` —
/// stdio is inherited so the command can print progress or ask questions.
pub struct ShellSetup;

impl WorktreeSetup for ShellSetup {
    fn run_command(&self, worktree: &Path, main_root: &Path, command: &str) -> Result<(), String> {
        let (program, args) = shell_argv(command, cfg!(windows));
        let status = std::process::Command::new(program)
            .args(&args)
            .current_dir(worktree)
            .env("ROOT_WORKTREE_PATH", main_root)
            .status()
            .map_err(|e| format!("failed to run '{command}': {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("'{command}' exited with {status}"))
        }
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Cursor<'_> {
    fn error(&self, what: &str) -> String {
        format!("{what} at byte {}", self.pos)
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), String> {
        if self.peek() == Some(byte) {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.error(&format!("expected '{}'", byte as char)))
        }
    }

    /// A loop body is only re-entered after a ',', so the closing bracket here is
    /// a trailing comma. Say that instead of blaming whatever the comma promised:
    /// `JSON.parse` — what the other tools reading this file use — rejects it too,
    /// so the fix is to delete the comma, not to write a value after it.
    fn reject_trailing_comma(&self, closer: u8) -> Result<(), String> {
        if self.peek() == Some(closer) {
            return Err(self.error(&format!("trailing comma before '{}'", closer as char)));
        }
        Ok(())
    }

    fn object_setup_key(&mut self) -> Result<Vec<String>, String> {
        self.expect(b'{')?;
        let mut commands = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(commands);
        }
        loop {
            self.skip_ws();
            self.reject_trailing_comma(b'}')?;
            let key = self.string()?;
            self.skip_ws();
            self.expect(b':')?;
            self.skip_ws();
            if key == "setup-worktree" {
                commands = self.string_array("setup-worktree")?;
            } else {
                self.skip_value()?;
            }
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(commands);
                }
                _ => return Err(self.error("expected ',' or '}'")),
            }
        }
    }

    fn string_array(&mut self, what: &str) -> Result<Vec<String>, String> {
        if self.peek() != Some(b'[') {
            return Err(self.error(&format!("{what} must be an array of strings")));
        }
        self.pos += 1;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(items);
        }
        loop {
            self.skip_ws();
            self.reject_trailing_comma(b']')?;
            if self.peek() != Some(b'"') {
                return Err(self.error(&format!("{what} must contain only strings")));
            }
            items.push(self.string()?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(items);
                }
                _ => return Err(self.error("expected ',' or ']'")),
            }
        }
    }

    fn skip_value(&mut self) -> Result<(), String> {
        match self.peek() {
            Some(b'"') => self.string().map(|_| ()),
            Some(b'[') => {
                self.pos += 1;
                self.skip_ws();
                if self.peek() == Some(b']') {
                    self.pos += 1;
                    return Ok(());
                }
                loop {
                    self.skip_ws();
                    self.reject_trailing_comma(b']')?;
                    self.skip_value()?;
                    self.skip_ws();
                    match self.peek() {
                        Some(b',') => self.pos += 1,
                        Some(b']') => {
                            self.pos += 1;
                            return Ok(());
                        }
                        _ => return Err(self.error("expected ',' or ']'")),
                    }
                }
            }
            Some(b'{') => {
                self.pos += 1;
                self.skip_ws();
                if self.peek() == Some(b'}') {
                    self.pos += 1;
                    return Ok(());
                }
                loop {
                    self.skip_ws();
                    self.reject_trailing_comma(b'}')?;
                    self.string()?;
                    self.skip_ws();
                    self.expect(b':')?;
                    self.skip_ws();
                    self.skip_value()?;
                    self.skip_ws();
                    match self.peek() {
                        Some(b',') => self.pos += 1,
                        Some(b'}') => {
                            self.pos += 1;
                            return Ok(());
                        }
                        _ => return Err(self.error("expected ',' or '}'")),
                    }
                }
            }
            Some(b't') => self.literal("true"),
            Some(b'f') => self.literal("false"),
            Some(b'n') => self.literal("null"),
            Some(b'-' | b'0'..=b'9') => {
                while matches!(self.peek(), Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')) {
                    self.pos += 1;
                }
                Ok(())
            }
            _ => Err(self.error("expected a JSON value")),
        }
    }

    fn literal(&mut self, word: &str) -> Result<(), String> {
        if self.bytes[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(())
        } else {
            Err(self.error(&format!("expected '{word}'")))
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            let Some(b) = self.peek() else {
                return Err(self.error("unterminated string"));
            };
            self.pos += 1;
            match b {
                b'"' => return Ok(out),
                b'\\' => {
                    let Some(e) = self.peek() else {
                        return Err(self.error("unterminated escape"));
                    };
                    self.pos += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'n' => out.push('\n'),
                        b't' => out.push('\t'),
                        b'r' => out.push('\r'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'u' => {
                            let hex = self.bytes.get(self.pos..self.pos + 4)
                                .and_then(|h| std::str::from_utf8(h).ok())
                                .and_then(|h| u32::from_str_radix(h, 16).ok())
                                .and_then(char::from_u32)
                                .ok_or_else(|| self.error("invalid \\u escape"))?;
                            self.pos += 4;
                            out.push(hex);
                        }
                        _ => return Err(self.error("invalid escape")),
                    }
                }
                _ if b < 0x20 => return Err(self.error("control character in string")),
                _ => {
                    let start = self.pos - 1;
                    let len = utf8_len(b);
                    let chunk = self.bytes.get(start..start + len)
                        .and_then(|c| std::str::from_utf8(c).ok())
                        .ok_or_else(|| self.error("invalid UTF-8"))?;
                    out.push_str(chunk);
                    self.pos = start + len;
                }
            }
        }
    }
}

fn utf8_len(first: u8) -> usize {
    match first {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_a_top_level_array() {
        assert_eq!(parse(r#"["npm install", "echo done"]"#).unwrap(), vec!["npm install", "echo done"]);
    }

    #[test]
    fn parse_accepts_the_setup_worktree_key_and_ignores_other_keys() {
        let json = r#"{
          "name": "x", "nested": {"a": [1, 2.5, -3e2, true, false, null, "s"], "b": {}},
          "setup-worktree": ["fvm use", "dart pub get", "cd tools/shuttel_lint && dart pub get && cd .."],
          "trailing": []
        }"#;
        assert_eq!(parse(json).unwrap(), vec!["fvm use", "dart pub get", "cd tools/shuttel_lint && dart pub get && cd .."]);
    }

    #[test]
    fn parse_unescapes_strings() {
        let json = r#"["cp \"$ROOT_WORKTREE_PATH/a b\" .", "a\\b", "tab\there", "slash\/", "nl\n", "cr\r", "bs\b", "ff\f", "A\u00e9", "Aé€😀"]"#;
        assert_eq!(parse(json).unwrap(), vec!["cp \"$ROOT_WORKTREE_PATH/a b\" .", "a\\b", "tab\there", "slash/", "nl\n", "cr\r", "bs\u{8}", "ff\u{c}", "Aé", "Aé€😀"]);
    }

    #[test]
    fn parse_rejects_non_string_entries() {
        let err = parse(r#"{"setup-worktree": ["ok", 42]}"#).unwrap_err();
        assert!(err.contains("setup-worktree") && err.contains("string"), "got: {err}");
    }

    #[test]
    fn parse_rejects_invalid_json_and_wrong_top_level() {
        assert!(parse(r#"["unterminated]"#).is_err());
        assert!(parse(r#"{"setup-worktree": ["a"]"#).is_err());
        assert!(parse(r#""just a string""#).is_err());
        assert!(parse(r#"{"setup-worktree": "not an array"}"#).is_err());
        assert!(parse(r#"["a"] trailing"#).is_err());
        assert!(parse(r#"{"k" 1}"#).is_err());
        assert!(parse(r#"[tru]"#).is_err());
        assert!(parse(r#"["bad \q escape"]"#).is_err());
        assert!(parse(r#"["bad \u12"]"#).is_err());
        assert!(parse("[\"ctrl\u{1}\"]").is_err());
        assert!(parse("[\"dangling\\").is_err());
        assert!(parse("[\"open").is_err());
        assert!(parse(r#"{"k": [1 2]}"#).is_err());
        assert!(parse(r#"{"k": {"a": 1 "b": 2}}"#).is_err());
        assert!(parse(r#"{"k": ?}"#).is_err());
        assert!(parse(r#"["a" "b"]"#).unwrap_err().contains("expected ',' or ']'"));
        assert!(parse(r#"{"k": tru}"#).unwrap_err().contains("expected 'true'"));
        assert!(parse(r#"{"k": {}, "setup-worktree": [], "x": [[], {}, ""]}"#).is_ok());
        assert!(parse(r#"{}"#).is_ok());
        assert!(parse("").is_err());
    }

    #[test]
    fn parse_of_an_empty_array_yields_no_commands() {
        assert_eq!(parse("[]").unwrap(), Vec::<String>::new());
        assert_eq!(parse(r#"{"other": 1}"#).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn shell_argv_is_sh_c_on_unix_and_cmd_c_on_windows() {
        assert_eq!(shell_argv("dart pub get", false), ("sh", vec!["-c".to_string(), "dart pub get".to_string()]));
        assert_eq!(shell_argv("dart pub get", true), ("cmd", vec!["/C".to_string(), "dart pub get".to_string()]));
    }

    #[test]
    fn load_prefers_cursor_file_then_falls_back_then_none() {
        let dir = crate::test_support::tmp_dir("gflow-worktree-setup-test");
        assert_eq!(load(&dir).unwrap(), None);

        std::fs::write(dir.join(GENERIC_FILE), r#"{"setup-worktree": ["generic"]}"#).unwrap();
        assert_eq!(load(&dir).unwrap(), Some(SetupCommands { file: dir.join(GENERIC_FILE), commands: vec!["generic".into()] }));

        std::fs::create_dir_all(dir.join(".cursor")).unwrap();
        std::fs::write(dir.join(CURSOR_FILE), "[]").unwrap();
        assert_eq!(load(&dir).unwrap().unwrap().commands, vec!["generic".to_string()], "an empty cursor file falls through");

        std::fs::write(dir.join(CURSOR_FILE), r#"["cursor"]"#).unwrap();
        assert_eq!(load(&dir).unwrap().unwrap(), SetupCommands { file: dir.join(CURSOR_FILE), commands: vec!["cursor".into()] });
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn parse_names_a_trailing_comma_rather_than_blaming_what_follows_it() {
        for json in [
            r#"["a",]"#,
            r#"{"setup-worktree": ["a",]}"#,
            r#"{"setup-worktree": ["a"],}"#,
            r#"{"other": [1,], "setup-worktree": []}"#,
            r#"{"other": {"a": 1,}, "setup-worktree": []}"#,
        ] {
            let err = parse(json).unwrap_err();
            assert!(err.contains("trailing comma"), "{json} got: {err}");
        }
    }

    #[test]
    fn resolve_ignores_the_file_when_the_worktree_flow_is_off() {
        let dir = crate::test_support::tmp_dir("gflow-worktree-setup-test");
        std::fs::write(dir.join(GENERIC_FILE), r#"{"setup-worktree": ["a",]}"#).unwrap();
        assert_eq!(resolve(&dir, false), (None, None));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resolve_yields_the_commands_when_the_file_parses() {
        let dir = crate::test_support::tmp_dir("gflow-worktree-setup-test");
        std::fs::write(dir.join(GENERIC_FILE), r#"{"setup-worktree": ["a"]}"#).unwrap();
        let (commands, warning) = resolve(&dir, true);
        assert_eq!(commands.unwrap().commands, vec!["a".to_string()]);
        assert_eq!(warning, None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resolve_warns_instead_of_failing_when_the_file_is_unparsable() {
        let dir = crate::test_support::tmp_dir("gflow-worktree-setup-test");
        std::fs::write(dir.join(GENERIC_FILE), r#"{"setup-worktree": ["a",]}"#).unwrap();
        let (commands, warning) = resolve(&dir, true);
        assert_eq!(commands, None);
        let warning = warning.expect("a broken setup file must warn");
        assert!(warning.contains("worktrees.json"), "got: {warning}");
        assert!(warning.contains("trailing comma"), "got: {warning}");
        assert!(warning.contains("Setup commands skipped"), "got: {warning}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_names_the_file_on_a_parse_error() {
        let dir = crate::test_support::tmp_dir("gflow-worktree-setup-test");
        std::fs::write(dir.join(GENERIC_FILE), "{ nope").unwrap();
        let err = load(&dir).unwrap_err();
        assert!(err.contains("worktrees.json"), "got: {err}");
        std::fs::remove_dir_all(&dir).ok();
    }
}

#![cfg(any(target_os = "linux", target_os = "macos"))]

use std::ffi::{c_char, c_int, c_ulong, c_void};
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use gflow::menu::MenuPrompter;
use gflow::prompt::Prompter;

#[cfg_attr(target_os = "linux", link(name = "util"))]
extern "C" {
    fn openpty(
        master: *mut c_int,
        slave: *mut c_int,
        name: *mut c_char,
        termios: *const c_void,
        winsize: *const c_void,
    ) -> c_int;
    fn setsid() -> c_int;
    fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
    fn fcntl(fd: c_int, command: c_int, ...) -> c_int;
}

#[cfg(target_os = "macos")]
const CONTROLLING_TERMINAL: c_ulong = 0x20007461;
#[cfg(target_os = "linux")]
const CONTROLLING_TERMINAL: c_ulong = 0x540e;

#[cfg(target_os = "macos")]
const NONBLOCK: c_int = 4;
#[cfg(target_os = "linux")]
const NONBLOCK: c_int = 0x800;

struct TerminalChild(Child);

impl Drop for TerminalChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn read_available(master: &mut File, output: &mut Vec<u8>) {
    loop {
        let mut buffer = [0; 4096];
        match master.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => output.extend_from_slice(&buffer[..count]),
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.raw_os_error() == Some(5) =>
            {
                break
            }
            Err(error) => panic!("cannot read controlled terminal: {error}"),
        }
    }
}

#[test]
fn real_terminal_adapter_selects_and_reads_without_leaking_raw_mode() {
    let (mut master_fd, mut slave_fd) = (-1, -1);
    assert_eq!(
        unsafe {
            openpty(
                &mut master_fd,
                &mut slave_fd,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
            )
        },
        0
    );
    let mut master = unsafe { File::from_raw_fd(master_fd) };
    let slave = unsafe { OwnedFd::from_raw_fd(slave_fd) };
    assert_ne!(unsafe { fcntl(master_fd, 4, NONBLOCK) }, -1);
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--ignored", "--exact", "terminal_child", "--nocapture"])
        .env("GFLOW_TERMINAL_CHILD", "1")
        .stdin(Stdio::from(slave.try_clone().unwrap()))
        .stdout(Stdio::from(slave.try_clone().unwrap()))
        .stderr(Stdio::from(slave.try_clone().unwrap()));
    unsafe {
        command.pre_exec(|| {
            if setsid() == -1 || ioctl(0, CONTROLLING_TERMINAL, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = TerminalChild(command.spawn().unwrap());
    drop(command);
    drop(slave);

    let script: &[(&str, &[u8])] = &[
        ("> 1) first", b"\x1b[B\r"),
        ("Name: ", b" --release  fix-- \r"),
        ("Command: ", b"  ~/my folder/tool --flag  \r"),
    ];
    let mut output = Vec::new();
    let mut next_input = 0;
    let started = Instant::now();
    let status = loop {
        read_available(&mut master, &mut output);
        if let Some((prompt, input)) = script.get(next_input) {
            if String::from_utf8_lossy(&output).contains(prompt) {
                master.write_all(input).unwrap();
                next_input += 1;
            }
        }
        if let Some(status) = child.0.try_wait().unwrap() {
            read_available(&mut master, &mut output);
            break status;
        }
        if started.elapsed() > Duration::from_secs(10) {
            panic!(
                "controlled terminal timed out: {}",
                String::from_utf8_lossy(&output)
            );
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    let output = String::from_utf8_lossy(&output);
    assert!(status.success(), "terminal child failed: {output}");
    assert_eq!(
        next_input,
        script.len(),
        "all three prompts must run: {output}"
    );
    assert!(
        output.contains("> 2) second"),
        "chosen entry must be highlighted: {output}"
    );
    assert!(
        output.matches("\x1b[?25h").count() >= 3,
        "each prompt must restore the cursor: {output}"
    );
}

#[test]
#[ignore = "runs only inside the controlled terminal test"]
fn terminal_child() {
    assert_eq!(std::env::var("GFLOW_TERMINAL_CHILD").as_deref(), Ok("1"));
    let prompter = MenuPrompter;
    assert_eq!(prompter.select("Pick", &["first", "second"]), Ok(1));
    assert!(!crossterm::terminal::is_raw_mode_enabled().unwrap());
    assert_eq!(prompter.prompt_name("Name"), Ok("release-fix".into()));
    assert!(!crossterm::terminal::is_raw_mode_enabled().unwrap());
    assert_eq!(
        prompter.prompt_line("Command"),
        Ok("~/my folder/tool --flag".into())
    );
    assert!(!crossterm::terminal::is_raw_mode_enabled().unwrap());
}

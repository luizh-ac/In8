//! Commands are always a program plus a list of arguments. Nothing here builds a shell string.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Cmd {
    pub program: String,
    pub args: Vec<String>,
    /// Extra environment variables for the process.
    pub env: Vec<(String, String)>,
    /// Text written to the process's stdin (e.g. file contents for `tee`).
    pub input: Option<String>,
    /// Working directory; the current one when `None`.
    pub cwd: Option<PathBuf>,
}

impl Cmd {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            env: Vec::new(),
            input: None,
            cwd: None,
        }
    }

    pub fn cwd(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cwd = Some(dir.into());
        self
    }

    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    pub fn input(mut self, input: impl Into<String>) -> Self {
        self.input = Some(input.into());
        self
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Readable form for logs and dry runs. Never used to execute anything.
    pub fn display(&self) -> String {
        std::iter::once(self.program.as_str())
            .chain(self.args.iter().map(String::as_str))
            .map(quote_for_display)
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// A process with stdout/stderr piped and stdin piped only when there is input,
    /// without a console window on Windows.
    pub fn to_command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command
            .args(&self.args)
            .envs(self.env.iter().map(|(key, value)| (key, value)))
            .stdin(if self.input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(dir) = &self.cwd {
            command.current_dir(dir);
        }
        hide_console_window(&mut command);
        command
    }

    /// A process in its own console window, for programs the user types into (e.g. `ssh-keygen` asking
    /// for a passphrase). Nothing is piped, so In8 never sees what is typed.
    pub fn to_console_command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command
            .args(&self.args)
            .envs(self.env.iter().map(|(key, value)| (key, value)));
        if let Some(dir) = &self.cwd {
            command.current_dir(dir);
        }
        new_console_window(&mut command);
        command
    }
}

/// Finds a program that may have been installed after In8 started (its PATH is from that moment):
/// the PATH first, then the folders its Windows installer uses. Falls back to the bare name.
pub fn locate(program: &str) -> String {
    if is_on_path(program) {
        return program.to_string();
    }
    known_locations(program)
        .into_iter()
        .find(|path| path.is_file())
        .map_or_else(
            || program.to_string(),
            |path| path.to_string_lossy().into_owned(),
        )
}

pub fn is_on_path(program: &str) -> bool {
    let names: Vec<String> = if cfg!(windows) {
        vec![format!("{program}.exe"), format!("{program}.cmd")]
    } else {
        vec![program.to_string()]
    };
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|dir| names.iter().any(|name| dir.join(name).is_file()))
    })
}

fn known_locations(program: &str) -> Vec<PathBuf> {
    if !cfg!(windows) {
        return Vec::new();
    }
    let env_dir = |key: &str| std::env::var_os(key).map(PathBuf::from);
    match program {
        "git" => ["ProgramFiles", "ProgramW6432"]
            .iter()
            .filter_map(|key| env_dir(key))
            .map(|dir| dir.join("Git").join("cmd").join("git.exe"))
            .collect(),
        "rustup" => env_dir("USERPROFILE")
            .map(|home| home.join(".cargo").join("bin").join("rustup.exe"))
            .into_iter()
            .collect(),
        _ => Vec::new(),
    }
}

/// The user's home folder.
pub fn home_dir() -> Option<PathBuf> {
    let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(key).map(PathBuf::from)
}

/// Runs a short command and returns its stdout if it exits successfully.
pub fn capture(cmd: &Cmd) -> Option<String> {
    let output = cmd.to_command().output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Runs a short command and returns its stdout whatever the exit code (`None` if it couldn't start).
pub fn capture_stdout(cmd: &Cmd) -> Option<String> {
    let output = cmd.to_command().output().ok()?;
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn quote_for_display(part: &str) -> String {
    if !part.is_empty() && !part.chars().any(|c| c.is_whitespace() || c == '"') {
        part.to_string()
    } else {
        format!("\"{}\"", part.replace('"', "\\\""))
    }
}

#[cfg(windows)]
fn hide_console_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console_window(_command: &mut Command) {}

#[cfg(windows)]
fn new_console_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
    command.creation_flags(CREATE_NEW_CONSOLE);
}

/// Linux has no console windows to open; the program inherits the terminal In8 was started from, if any.
#[cfg(not(windows))]
fn new_console_window(_command: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_quotes_only_when_needed() {
        let cmd = Cmd::new("winget").args(["install", "--id", "Some App", "say \"hi\"", ""]);
        assert_eq!(
            cmd.display(),
            r#"winget install --id "Some App" "say \"hi\"" """#
        );
    }

    #[test]
    fn builder_keeps_arguments_separate() {
        let cmd = Cmd::new("git")
            .arg("config")
            .args(["--global", "user.name", "A; rm -rf /"]);
        assert_eq!(cmd.args, ["config", "--global", "user.name", "A; rm -rf /"]);
    }
}

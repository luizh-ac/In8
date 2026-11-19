//! Commands are always a program plus a list of arguments. Nothing here builds a shell string.

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
}

impl Cmd {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            env: Vec::new(),
            input: None,
        }
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
        hide_console_window(&mut command);
        command
    }
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

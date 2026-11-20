//! Root on native Linux with one password per run.
//!
//! The first task that needs root starts `pkexec <in8> --root-helper`: a copy of In8 running as root
//! that reads one command per line (JSON) from its stdin, runs it if it's on the allowlist below and
//! streams the output back. Only the In8 that started it holds that pipe. The helper ends when the run
//! ends (stdin closes). It never runs a shell, and accepts only package installs with fixed shapes.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc;
use std::thread;

use serde::{Deserialize, Serialize};

use crate::exec::Cmd;

/// Command-line flag that starts the helper instead of the app.
pub const HELPER_FLAG: &str = "--root-helper";

/// One command for the helper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Request {
    program: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
}

/// What the helper sends back, one JSON object per line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Message {
    /// Authentication worked and the helper is waiting for commands.
    Ready,
    Line {
        line: String,
    },
    Exit {
        code: Option<i32>,
    },
    Rejected {
        reason: String,
    },
}

fn is_package_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && name.len() <= 100
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "+-._".contains(c))
}

/// The only commands the helper runs: these exact flags followed by package names.
fn check(request: &Request) -> Result<(), String> {
    let env_ok = request
        .env
        .iter()
        .all(|(key, value)| key == "DEBIAN_FRONTEND" && value == "noninteractive");
    if !env_ok {
        return Err("environment variables aren't allowed".into());
    }
    let args: Vec<&str> = request.args.iter().map(String::as_str).collect();
    let packages = match (request.program.as_str(), args.as_slice()) {
        ("apt-get", ["update"]) => return Ok(()),
        ("apt-get", ["install", "-y", packages @ ..]) => packages,
        ("dnf", ["install", "-y", packages @ ..]) => packages,
        ("pacman", ["-Syu", "--needed", "--noconfirm", packages @ ..]) => packages,
        _ => return Err(format!("{} isn't allowed", request.program)),
    };
    if packages.is_empty() || !packages.iter().all(|p| is_package_name(p)) {
        return Err("bad package name".into());
    }
    Ok(())
}

fn send(out: &mut impl Write, message: &Message) -> std::io::Result<()> {
    let mut line = serde_json::to_string(message).map_err(std::io::Error::other)?;
    line.push('\n');
    out.write_all(line.as_bytes())?;
    out.flush()
}

/// Entry point of `in8 --root-helper` (runs as root). Returns the process exit code.
pub fn run_helper() -> i32 {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    if send(&mut out, &Message::Ready).is_err() {
        return 1;
    }
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let reply = match serde_json::from_str::<Request>(&line) {
            Ok(request) => match check(&request) {
                Ok(()) => run_checked(&request, &mut out),
                Err(reason) => send(&mut out, &Message::Rejected { reason }),
            },
            Err(_) => send(
                &mut out,
                &Message::Rejected {
                    reason: "unreadable request".into(),
                },
            ),
        };
        if reply.is_err() {
            return 1;
        }
    }
    0
}

fn run_checked(request: &Request, out: &mut impl Write) -> std::io::Result<()> {
    let child = Command::new(&request.program)
        .args(&request.args)
        .envs(request.env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(error) => {
            send(
                out,
                &Message::Line {
                    line: format!("Couldn't start {}: {error}", request.program),
                },
            )?;
            return send(out, &Message::Exit { code: None });
        }
    };
    let (tx, rx) = mpsc::channel::<String>();
    let streams: Vec<Box<dyn Read + Send>> = [
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .flatten()
    .collect();
    for stream in streams {
        let tx = tx.clone();
        thread::spawn(move || {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
    }
    drop(tx);
    for line in rx {
        send(out, &Message::Line { line })?;
    }
    let code = child.wait().ok().and_then(|status| status.code());
    send(out, &Message::Exit { code })
}

/// The In8 side: a running helper, started on the first root task of a run.
pub struct RootSession {
    child: Child,
    /// Dropped first when the session ends, which tells the helper to exit.
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl RootSession {
    /// Asks for the password (pkexec's own dialog) and waits until the helper is ready.
    pub fn start() -> Result<Self, String> {
        // Inside an AppImage, root can't read the user's mount, so pkexec gets the AppImage file itself.
        let exe = std::env::var_os("APPIMAGE")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::current_exe().ok())
            .ok_or("couldn't find In8's own executable")?;
        let mut child = Command::new("pkexec")
            .arg(exe)
            .arg(HELPER_FLAG)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    "pkexec isn't installed, so In8 can't ask for the administrator password. Install it \
                     with your distro's package manager (Debian/Ubuntu: pkexec; Fedora and Arch: polkit), \
                     then try again."
                        .to_string()
                } else {
                    format!("Couldn't start pkexec: {e}")
                }
            })?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            return Err("couldn't talk to the administrator helper".into());
        };
        let mut session = Self {
            child,
            stdin: Some(stdin),
            stdout: BufReader::new(stdout),
        };
        match session.next_message() {
            Some(Message::Ready) => Ok(session),
            _ => {
                let mut error = String::new();
                if let Some(mut stderr) = session.child.stderr.take() {
                    let _ = stderr.read_to_string(&mut error);
                }
                let _ = session.child.wait();
                let error = error.trim();
                if error.contains("authentication agent") {
                    return Err("There's no password dialog (polkit agent) in this session, so In8 can't ask \
                         for the administrator password. This happens in WSL and on minimal desktops."
                        .into());
                }
                Err(if error.is_empty() {
                    "The administrator password wasn't given.".to_string()
                } else {
                    format!("The administrator password wasn't given ({error}).")
                })
            }
        }
    }

    fn next_message(&mut self) -> Option<Message> {
        let mut line = String::new();
        match self.stdout.read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => serde_json::from_str(line.trim()).ok(),
        }
    }

    /// Runs one command as root, passing each output line to `log`. `None` if it couldn't run.
    pub fn run(&mut self, cmd: &Cmd, log: &mut dyn FnMut(String)) -> Option<i32> {
        let request = Request {
            program: cmd.program.clone(),
            args: cmd.args.clone(),
            env: cmd.env.clone(),
        };
        let mut line = serde_json::to_string(&request).ok()?;
        line.push('\n');
        let written = self.stdin.as_mut().is_some_and(|stdin| {
            stdin
                .write_all(line.as_bytes())
                .and_then(|()| stdin.flush())
                .is_ok()
        });
        if !written {
            log("The administrator helper stopped.".into());
            return None;
        }
        loop {
            match self.next_message() {
                Some(Message::Line { line }) => log(line),
                Some(Message::Exit { code }) => return code,
                Some(Message::Rejected { reason }) => {
                    log(format!("Refused by the administrator helper: {reason}"));
                    return None;
                }
                Some(Message::Ready) => {}
                None => {
                    log("The administrator helper stopped.".into());
                    return None;
                }
            }
        }
    }
}

impl Drop for RootSession {
    /// Closing stdin ends the helper's loop.
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pm::{apt, dnf, pacman};

    fn request(cmd: &Cmd) -> Request {
        Request {
            program: cmd.program.clone(),
            args: cmd.args.clone(),
            env: cmd.env.clone(),
        }
    }

    #[test]
    fn accepts_the_install_commands_in8_builds() {
        assert!(check(&request(&apt::native_update_cmd())).is_ok());
        assert!(check(&request(&apt::native_install_cmd(&["git", "python3"]))).is_ok());
        assert!(check(&request(&dnf::install_cmd(&["git"]))).is_ok());
        assert!(check(&request(&pacman::install_cmd(&["nodejs-lts-krypton"]))).is_ok());
    }

    #[test]
    fn rejects_anything_else() {
        let reject = |program: &str, args: &[&str], env: &[(&str, &str)]| {
            check(&Request {
                program: program.into(),
                args: args.iter().map(|a| a.to_string()).collect(),
                env: env
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
            })
            .is_err()
        };
        assert!(reject("sh", &["-c", "id"], &[]));
        assert!(reject("apt-get", &["remove", "-y", "git"], &[]));
        assert!(reject("apt-get", &["install", "-y"], &[]));
        assert!(reject("apt-get", &["install", "-y", "-o", "APT::x=y"], &[]));
        assert!(reject("apt-get", &["install", "-y", "./local.deb"], &[]));
        assert!(reject("dnf", &["install", "-y", "git;id"], &[]));
        assert!(reject("pacman", &["-U", "file.pkg"], &[]));
        assert!(reject(
            "apt-get",
            &["update"],
            &[("LD_PRELOAD", "/tmp/x.so")]
        ));
        assert!(reject("/usr/bin/apt-get", &["update"], &[]));
    }

    #[test]
    fn messages_round_trip_as_json_lines() {
        let line = serde_json::to_string(&Message::Line { line: "hi".into() }).unwrap();
        assert_eq!(line, r#"{"type":"line","line":"hi"}"#);
        let exit: Message = serde_json::from_str(r#"{"type":"exit","code":0}"#).unwrap();
        assert_eq!(exit, Message::Exit { code: Some(0) });
    }
}

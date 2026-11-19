//! Builds the task queue and runs it, streaming output. Every install goes through here,
//! and every run can be a dry run (commands shown, nothing executed).

use std::collections::HashSet;
use std::io::Read;
use std::sync::mpsc;
use std::thread;

use serde::Serialize;

use crate::catalog::Catalog;
use crate::error::{Error, Result};
use crate::exec::Cmd;
use crate::platform::PlatformInfo;
use crate::pm::{self, InstallOutcome, PmId};
use crate::resolve::{Resolution, resolve};

#[derive(Debug, Clone)]
pub struct Task {
    pub id: String,
    pub label: String,
    pub cmd: Cmd,
    pub kind: TaskKind,
}

#[derive(Debug, Clone, Copy)]
pub enum TaskKind {
    Setup,
    Install(PmId),
}

/// Mirrored in `src/lib/types.ts` (`TaskStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskStatus {
    Succeeded,
    AlreadyInstalled,
    Failed,
    Simulated,
}

/// Mirrored in `src/lib/types.ts` (`RunEvent`).
#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "event",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RunEvent {
    TaskStarted {
        task_id: String,
        label: String,
        command: String,
    },
    Output {
        task_id: String,
        line: String,
    },
    TaskFinished {
        task_id: String,
        status: TaskStatus,
        exit_code: Option<i32>,
    },
    Done {
        succeeded: usize,
        failed: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RunSummary {
    pub succeeded: usize,
    pub failed: usize,
}

/// Turns selected app IDs into tasks: one-time setup steps first, then installs in selection order.
pub fn plan_install(
    catalog: &Catalog,
    platform: &PlatformInfo,
    app_ids: &[String],
) -> Result<Vec<Task>> {
    let mut seen = HashSet::new();
    let mut managers = Vec::new();
    let mut installs = Vec::new();

    for id in app_ids {
        if !seen.insert(id.as_str()) {
            continue;
        }
        let app = catalog
            .get(id)
            .ok_or_else(|| Error::UnknownApp(id.clone()))?;
        let Resolution::Available { manager, .. } = resolve(app, platform) else {
            return Err(Error::Unavailable(app.name.clone()));
        };
        let cmd =
            pm::install_cmd(manager, app).ok_or_else(|| Error::Unavailable(app.name.clone()))?;
        if !managers.contains(&manager) {
            managers.push(manager);
        }
        installs.push(Task {
            id: app.id.clone(),
            label: app.name.clone(),
            cmd,
            kind: TaskKind::Install(manager),
        });
    }

    let setups = managers.into_iter().filter_map(|manager| {
        pm::setup_step(manager).map(|step| Task {
            id: format!("setup-{}", manager.as_str()),
            label: step.label.to_string(),
            cmd: step.cmd,
            kind: TaskKind::Setup,
        })
    });
    Ok(setups.chain(installs).collect())
}

pub fn run(tasks: &[Task], dry_run: bool, mut emit: impl FnMut(RunEvent)) -> RunSummary {
    let (mut succeeded, mut failed) = (0, 0);
    for task in tasks {
        emit(RunEvent::TaskStarted {
            task_id: task.id.clone(),
            label: task.label.clone(),
            command: task.cmd.display(),
        });
        let (status, exit_code) = if dry_run {
            (TaskStatus::Simulated, None)
        } else {
            execute(task, &mut emit)
        };
        if status == TaskStatus::Failed {
            failed += 1;
        } else {
            succeeded += 1;
        }
        emit(RunEvent::TaskFinished {
            task_id: task.id.clone(),
            status,
            exit_code,
        });
    }
    emit(RunEvent::Done { succeeded, failed });
    RunSummary { succeeded, failed }
}

fn execute(task: &Task, emit: &mut impl FnMut(RunEvent)) -> (TaskStatus, Option<i32>) {
    let mut child = match task.cmd.to_command().spawn() {
        Ok(child) => child,
        Err(error) => {
            emit(RunEvent::Output {
                task_id: task.id.clone(),
                line: format!("Couldn't start {}: {error}", task.cmd.program),
            });
            return (TaskStatus::Failed, None);
        }
    };

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

    let (tx, rx) = mpsc::channel::<String>();
    let readers: Vec<_> = streams
        .into_iter()
        .map(|stream| {
            let tx = tx.clone();
            thread::spawn(move || read_lines(stream, &tx))
        })
        .collect();
    drop(tx);

    for line in rx {
        emit(RunEvent::Output {
            task_id: task.id.clone(),
            line,
        });
    }
    for reader in readers {
        let _ = reader.join();
    }

    let code = child.wait().ok().and_then(|status| status.code());
    (status_for(task.kind, code), code)
}

fn status_for(kind: TaskKind, code: Option<i32>) -> TaskStatus {
    let outcome = match kind {
        TaskKind::Setup if code == Some(0) => InstallOutcome::Installed,
        TaskKind::Setup => InstallOutcome::Failed,
        TaskKind::Install(manager) => pm::interpret_exit(manager, code),
    };
    match outcome {
        InstallOutcome::Installed => TaskStatus::Succeeded,
        InstallOutcome::AlreadyInstalled => TaskStatus::AlreadyInstalled,
        InstallOutcome::Failed => TaskStatus::Failed,
    }
}

/// Splits on both `\n` and `\r` so progress updates that rewrite the same line become separate lines.
fn read_lines(mut stream: impl Read, tx: &mpsc::Sender<String>) {
    let mut buffer = [0u8; 4096];
    let mut pending = Vec::new();
    loop {
        let read = match stream.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        for &byte in &buffer[..read] {
            if byte == b'\n' || byte == b'\r' {
                flush_line(&mut pending, tx);
            } else {
                pending.push(byte);
            }
        }
    }
    flush_line(&mut pending, tx);
}

fn flush_line(pending: &mut Vec<u8>, tx: &mpsc::Sender<String>) {
    if pending.is_empty() {
        return;
    }
    if let Some(line) = normalize_line(&String::from_utf8_lossy(pending)) {
        let _ = tx.send(line);
    }
    pending.clear();
}

/// Drops spinner frames and progress bars that winget and others draw in place.
pub fn normalize_line(raw: &str) -> Option<String> {
    let line = raw.trim();
    if line.is_empty()
        || line
            .chars()
            .all(|c| matches!(c, '-' | '\\' | '|' | '/' | ' '))
        || line.contains(['█', '▒', '░'])
    {
        return None;
    }
    Some(line.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::Os;

    const CATALOG: &str = r#"{ "version": 1, "apps": [
        { "id": "git", "name": "Git", "description": "", "category": "dev",
          "packages": { "winget": "Git.Git", "apt": "git" } },
        { "id": "spotify", "name": "Spotify", "description": "", "category": "media",
          "packages": { "winget": "Spotify.Spotify", "flatpak": "com.spotify.Client" } }
    ] }"#;

    fn platform(os: Os, managers: &[PmId]) -> PlatformInfo {
        PlatformInfo {
            os,
            arch: "x86_64",
            version: "0.0.0",
            distro: None,
            managers: managers.to_vec(),
        }
    }

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn windows_plan_has_one_task_per_app_without_duplicates() {
        let catalog = Catalog::parse(CATALOG).unwrap();
        let tasks = plan_install(
            &catalog,
            &platform(Os::Windows, &[PmId::Winget]),
            &ids(&["spotify", "git", "spotify"]),
        )
        .unwrap();
        let task_ids: Vec<_> = tasks.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(task_ids, ["spotify", "git"]);
    }

    #[test]
    fn linux_plan_adds_flathub_setup_first() {
        let catalog = Catalog::parse(CATALOG).unwrap();
        let tasks = plan_install(
            &catalog,
            &platform(Os::Linux, &[PmId::Flatpak]),
            &ids(&["spotify"]),
        )
        .unwrap();
        let task_ids: Vec<_> = tasks.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(task_ids, ["setup-flatpak", "spotify"]);
    }

    #[test]
    fn plan_rejects_unknown_and_unavailable_apps() {
        let catalog = Catalog::parse(CATALOG).unwrap();
        let linux = platform(Os::Linux, &[PmId::Flatpak]);
        assert!(matches!(
            plan_install(&catalog, &linux, &ids(&["nope"])),
            Err(Error::UnknownApp(_))
        ));
        assert!(matches!(
            plan_install(&catalog, &linux, &ids(&["git"])),
            Err(Error::Unavailable(_))
        ));
    }

    #[test]
    fn dry_run_emits_commands_without_executing() {
        let tasks = vec![Task {
            id: "x".into(),
            label: "X".into(),
            cmd: Cmd::new("program-that-does-not-exist").arg("--flag"),
            kind: TaskKind::Setup,
        }];
        let mut events = Vec::new();
        let summary = run(&tasks, true, |event| events.push(event));
        assert_eq!(
            summary,
            RunSummary {
                succeeded: 1,
                failed: 0
            }
        );
        assert!(matches!(
            &events[0],
            RunEvent::TaskStarted { command, .. } if command == "program-that-does-not-exist --flag"
        ));
        assert!(matches!(
            events[1],
            RunEvent::TaskFinished {
                status: TaskStatus::Simulated,
                ..
            }
        ));
    }

    #[test]
    fn real_run_streams_output_and_reports_failure_to_start() {
        let tasks = vec![
            Task {
                id: "ok".into(),
                label: "cargo".into(),
                cmd: Cmd::new("cargo").arg("--version"),
                kind: TaskKind::Setup,
            },
            Task {
                id: "missing".into(),
                label: "missing".into(),
                cmd: Cmd::new("program-that-does-not-exist"),
                kind: TaskKind::Setup,
            },
        ];
        let mut events = Vec::new();
        let summary = run(&tasks, false, |event| events.push(event));
        assert_eq!(
            summary,
            RunSummary {
                succeeded: 1,
                failed: 1
            }
        );
        assert!(events.iter().any(|event| matches!(
            event,
            RunEvent::Output { task_id, line } if task_id == "ok" && line.starts_with("cargo ")
        )));
    }

    #[test]
    fn progress_noise_is_dropped() {
        assert_eq!(normalize_line("  -  "), None);
        assert_eq!(normalize_line("\\"), None);
        assert_eq!(normalize_line("  ██████▒▒▒▒  1.00 MB / 2.00 MB"), None);
        assert_eq!(normalize_line("   "), None);
        assert_eq!(
            normalize_line("  Successfully installed  "),
            Some("Successfully installed".to_string())
        );
    }
}

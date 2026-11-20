//! Builds the task queue and runs it, streaming output. Every install goes through here,
//! and every run can be a dry run (commands shown, nothing executed).

use std::collections::HashSet;
use std::io::{Read, Write};
use std::sync::mpsc;
use std::thread;

use serde::{Deserialize, Serialize};

use crate::catalog::{App, Catalog, Target};
use crate::error::{Error, Result};
use crate::exec::Cmd;
use crate::platform::{Os, PlatformInfo};
use crate::pm::{self, InstallAction, InstallOutcome, PmId, apt, github};
use crate::resolve::{Resolution, resolve, resolve_wsl};
use crate::wsl::{self, DistroDetails, WslAction};

#[derive(Debug, Clone)]
pub struct Task {
    pub id: String,
    pub label: String,
    pub action: InstallAction,
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
    /// A setup step (not an app install) that finished.
    Done,
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

/// What the user asked to run. Mirrored in `src/lib/types.ts` (`RunRequest`).
/// Also the shape of a profile's selection and of the backup file's selection.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRequest {
    #[serde(default)]
    pub apps: Vec<AppRequest>,
    #[serde(default)]
    pub wsl: Vec<WslAction>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppRequest {
    pub id: String,
    #[serde(default = "host_target")]
    pub target: Target,
}

fn host_target() -> Target {
    Target::Host
}

fn setup_task(id: impl Into<String>, label: impl Into<String>, cmd: Cmd) -> Task {
    Task {
        id: id.into(),
        label: label.into(),
        action: InstallAction::Command(cmd),
        kind: TaskKind::Setup,
    }
}

/// Turns a request into an ordered queue: host setup steps, host installs, then WSL
/// (WSL itself, distros, systemd, `apt-get update`, apps, Docker Engine), then VS Code's WSL extension.
/// Reads the distro's `/etc/wsl.conf` when systemd is requested, so run it off the main thread.
pub fn plan(
    catalog: &Catalog,
    platform: &PlatformInfo,
    distro: Option<&DistroDetails>,
    request: &RunRequest,
) -> Result<Vec<Task>> {
    let mut seen = HashSet::new();
    let mut host_setups: Vec<Task> = Vec::new();
    let mut host_installs = Vec::new();
    let mut wsl_apps: Vec<&App> = Vec::new();

    for wanted in &request.apps {
        if !seen.insert((wanted.id.as_str(), wanted.target)) {
            continue;
        }
        let app = catalog
            .get(&wanted.id)
            .ok_or_else(|| Error::UnknownApp(wanted.id.clone()))?;
        if wanted.target == Target::Wsl {
            wsl_apps.push(app);
            continue;
        }
        let Resolution::Available { manager, .. } = resolve(app, platform) else {
            return Err(Error::Unavailable(app.name.clone()));
        };
        let action =
            pm::install_action(manager, app).ok_or_else(|| Error::Unavailable(app.name.clone()))?;
        for step in pm::setup_steps(manager, app) {
            if !host_setups.iter().any(|task| task.id == step.id) {
                host_setups.push(setup_task(step.id, step.label, step.cmd));
            }
        }
        host_installs.push(Task {
            id: app.id.clone(),
            label: app.name.clone(),
            action,
            kind: TaskKind::Install(manager),
        });
    }

    let mut tasks = host_setups;
    tasks.extend(host_installs);
    tasks.extend(plan_wsl(platform, distro, &wsl_apps, &request.wsl)?);
    Ok(tasks)
}

fn plan_wsl(
    platform: &PlatformInfo,
    distro: Option<&DistroDetails>,
    apps: &[&App],
    actions: &[WslAction],
) -> Result<Vec<Task>> {
    if apps.is_empty() && actions.is_empty() {
        return Ok(Vec::new());
    }
    if platform.os != Os::Windows {
        return Err(Error::Wsl("WSL is only available on Windows".into()));
    }
    let wants = |action: &WslAction| actions.contains(action);
    let mut tasks = Vec::new();

    if wants(&WslAction::InstallWsl) {
        tasks.push(setup_task(
            "wsl-install",
            "Install WSL",
            wsl::wsl().args(["--install", "--no-distribution"]),
        ));
    }
    for action in actions {
        if let WslAction::InstallDistro { distro } = action {
            if !wsl::is_distro_name(distro) {
                return Err(Error::Wsl(format!("invalid distro name: {distro}")));
            }
            tasks.push(setup_task(
                format!("wsl-distro-{distro}"),
                format!("Install the {distro} distro"),
                wsl::wsl().args(["--install", "-d", distro, "--no-launch"]),
            ));
        }
    }

    let enable_systemd = wants(&WslAction::EnableSystemd);
    let docker = wants(&WslAction::DockerEngine);
    if !apps.is_empty() || enable_systemd || docker {
        let distro = distro.filter(|d| d.is_ready()).ok_or_else(|| {
            Error::Wsl("set up an Ubuntu or Debian distro with a Linux user first".into())
        })?;
        if docker && !distro.systemd && !enable_systemd {
            return Err(Error::Wsl(
                "Docker Engine needs systemd; turn it on too".into(),
            ));
        }
        let name = distro.name.as_str();

        if enable_systemd {
            tasks.push(setup_task(
                "wsl-systemd",
                "Turn on systemd in /etc/wsl.conf",
                wsl::inside(
                    name,
                    true,
                    Cmd::new("tee")
                        .arg("/etc/wsl.conf")
                        .input(wsl::wsl_conf_with_systemd(name)),
                ),
            ));
            tasks.push(setup_task(
                "wsl-restart",
                format!("Restart {name} so systemd starts"),
                wsl::wsl().args(["--terminate", name]),
            ));
        }
        if !apps.is_empty() || docker {
            tasks.push(setup_task(
                "wsl-apt-update",
                format!("Update package lists in {name}"),
                wsl::inside(name, true, apt::update_cmd()),
            ));
        }
        for app in apps {
            let Some(Resolution::Available { package, .. }) = resolve_wsl(app, Some(distro)) else {
                return Err(Error::Unavailable(format!("{} in WSL", app.name)));
            };
            tasks.push(Task {
                id: format!("{}@wsl", app.id),
                label: format!("{} (WSL)", app.name),
                action: InstallAction::Command(wsl::inside(
                    name,
                    true,
                    apt::install_cmd(&[&package]),
                )),
                kind: TaskKind::Install(PmId::Apt),
            });
        }
        if docker {
            tasks.push(Task {
                id: "docker-engine@wsl".into(),
                label: "Docker Engine (WSL)".into(),
                action: InstallAction::Command(wsl::inside(
                    name,
                    true,
                    apt::install_cmd(&["docker.io", "docker-compose-v2"]),
                )),
                kind: TaskKind::Install(PmId::Apt),
            });
            tasks.push(setup_task(
                "docker-group",
                format!("Let {} use Docker without sudo", distro.user),
                wsl::inside(
                    name,
                    true,
                    Cmd::new("usermod").args(["-aG", "docker", distro.user.as_str()]),
                ),
            ));
            tasks.push(setup_task(
                "docker-service",
                "Start Docker with the distro",
                wsl::inside(
                    name,
                    true,
                    Cmd::new("systemctl").args(["enable", "--now", "docker"]),
                ),
            ));
        }
    }

    if wants(&WslAction::VscodeExtension) {
        tasks.push(Task {
            id: "vscode-wsl-extension".into(),
            label: "VS Code WSL extension".into(),
            action: InstallAction::VscodeExtension(wsl::VSCODE_WSL_EXTENSION.into()),
            kind: TaskKind::Setup,
        });
    }
    Ok(tasks)
}

pub fn run(tasks: &[Task], dry_run: bool, mut emit: impl FnMut(RunEvent)) -> RunSummary {
    let (mut succeeded, mut failed) = (0, 0);
    for task in tasks {
        emit(RunEvent::TaskStarted {
            task_id: task.id.clone(),
            label: task.label.clone(),
            command: task.action.display(),
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
    let output = |line: String| RunEvent::Output {
        task_id: task.id.clone(),
        line,
    };
    let code = match &task.action {
        InstallAction::Command(cmd) => run_process(&task.id, cmd, emit),
        InstallAction::GithubRelease(package) => {
            // The downloaded file and its folder are deleted when `download` is dropped.
            let download = match github::download(package, |line| emit(output(line))) {
                Ok(download) => download,
                Err(message) => {
                    emit(output(message));
                    return (TaskStatus::Failed, None);
                }
            };
            let cmd = github::installer_cmd(package, &download.path);
            emit(output(format!("Running {}", cmd.display())));
            run_process(&task.id, &cmd, emit)
        }
        InstallAction::VscodeExtension(extension) => match wsl::find_code() {
            Some(code) => {
                let cmd = Cmd::new(code.to_string_lossy())
                    .args(["--install-extension", extension.as_str()]);
                run_process(&task.id, &cmd, emit)
            }
            None => {
                emit(output(
                    "VS Code wasn't found. Install it first, then try again.".into(),
                ));
                return (TaskStatus::Failed, None);
            }
        },
    };
    (status_for(task.kind, code), code)
}

/// Runs a process, streaming stdout and stderr as output events. `None` if it couldn't start.
fn run_process(task_id: &str, cmd: &Cmd, emit: &mut impl FnMut(RunEvent)) -> Option<i32> {
    let mut child = match cmd.to_command().spawn() {
        Ok(child) => child,
        Err(error) => {
            emit(RunEvent::Output {
                task_id: task_id.to_string(),
                line: format!("Couldn't start {}: {error}", cmd.program),
            });
            return None;
        }
    };

    if let (Some(input), Some(mut stdin)) = (cmd.input.clone(), child.stdin.take()) {
        // Written on its own thread so a full stdout pipe can't deadlock us; dropping stdin closes it.
        thread::spawn(move || {
            let _ = stdin.write_all(input.as_bytes());
        });
    }

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
            task_id: task_id.to_string(),
            line,
        });
    }
    for reader in readers {
        let _ = reader.join();
    }

    child.wait().ok().and_then(|status| status.code())
}

fn status_for(kind: TaskKind, code: Option<i32>) -> TaskStatus {
    let outcome = match kind {
        TaskKind::Setup if code == Some(0) => return TaskStatus::Done,
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

    const CATALOG: &str = r#"{ "version": 1, "apps": [
        { "id": "git", "name": "Git", "description": "", "category": "dev",
          "packages": { "winget": "Git.Git", "apt": "git" }, "targets": ["host", "wsl"] },
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

    fn windows() -> PlatformInfo {
        platform(Os::Windows, &[PmId::Winget, PmId::Github])
    }

    fn ubuntu(systemd: bool) -> DistroDetails {
        DistroDetails {
            name: "Ubuntu".into(),
            os_name: "Ubuntu 26.04 LTS".into(),
            user: "me".into(),
            has_user: true,
            systemd,
            apt: true,
            docker: false,
        }
    }

    fn apps(list: &[(&str, Target)]) -> RunRequest {
        RunRequest {
            apps: list
                .iter()
                .map(|(id, target)| AppRequest {
                    id: id.to_string(),
                    target: *target,
                })
                .collect(),
            wsl: Vec::new(),
        }
    }

    fn task_ids(tasks: &[Task]) -> Vec<&str> {
        tasks.iter().map(|t| t.id.as_str()).collect()
    }

    #[test]
    fn windows_plan_has_one_task_per_app_without_duplicates() {
        let catalog = Catalog::parse(CATALOG).unwrap();
        let request = apps(&[
            ("spotify", Target::Host),
            ("git", Target::Host),
            ("spotify", Target::Host),
        ]);
        let tasks = plan(&catalog, &windows(), None, &request).unwrap();
        assert_eq!(task_ids(&tasks), ["spotify", "git"]);
    }

    #[test]
    fn linux_plan_adds_flathub_setup_first() {
        let catalog = Catalog::parse(CATALOG).unwrap();
        let tasks = plan(
            &catalog,
            &platform(Os::Linux, &[PmId::Flatpak]),
            None,
            &apps(&[("spotify", Target::Host)]),
        )
        .unwrap();
        assert_eq!(task_ids(&tasks), ["setup-flatpak-flathub", "spotify"]);
    }

    #[test]
    fn plan_rejects_unknown_and_unavailable_apps() {
        let catalog = Catalog::parse(CATALOG).unwrap();
        let linux = platform(Os::Linux, &[PmId::Flatpak]);
        assert!(matches!(
            plan(&catalog, &linux, None, &apps(&[("nope", Target::Host)])),
            Err(Error::UnknownApp(_))
        ));
        assert!(matches!(
            plan(&catalog, &linux, None, &apps(&[("git", Target::Host)])),
            Err(Error::Unavailable(_))
        ));
    }

    #[test]
    fn same_app_can_go_to_windows_and_wsl() {
        let catalog = Catalog::parse(CATALOG).unwrap();
        let request = apps(&[("git", Target::Host), ("git", Target::Wsl)]);
        let tasks = plan(&catalog, &windows(), Some(&ubuntu(true)), &request).unwrap();
        assert_eq!(task_ids(&tasks), ["git", "wsl-apt-update", "git@wsl"]);
        let InstallAction::Command(cmd) = &tasks[2].action else {
            panic!("expected a command");
        };
        assert_eq!(
            cmd.args,
            [
                "-d",
                "Ubuntu",
                "-u",
                "root",
                "--exec",
                "env",
                "DEBIAN_FRONTEND=noninteractive",
                "apt-get",
                "install",
                "-y",
                "git"
            ]
        );
    }

    #[test]
    fn wsl_environment_runs_in_order() {
        let catalog = Catalog::parse(CATALOG).unwrap();
        let mut request = apps(&[("git", Target::Wsl), ("spotify", Target::Host)]);
        request.wsl = vec![
            WslAction::VscodeExtension,
            WslAction::DockerEngine,
            WslAction::InstallDistro {
                distro: "Ubuntu-26.04".into(),
            },
        ];
        let tasks = plan(&catalog, &windows(), Some(&ubuntu(true)), &request).unwrap();
        assert_eq!(
            task_ids(&tasks),
            [
                "spotify",
                "wsl-distro-Ubuntu-26.04",
                "wsl-apt-update",
                "git@wsl",
                "docker-engine@wsl",
                "docker-group",
                "docker-service",
                "vscode-wsl-extension"
            ]
        );
    }

    #[test]
    fn wsl_work_needs_a_ready_distro_and_docker_needs_systemd() {
        let catalog = Catalog::parse(CATALOG).unwrap();
        let wsl_git = apps(&[("git", Target::Wsl)]);
        assert!(matches!(
            plan(&catalog, &windows(), None, &wsl_git),
            Err(Error::Wsl(_))
        ));
        let mut no_user = ubuntu(true);
        no_user.has_user = false;
        assert!(plan(&catalog, &windows(), Some(&no_user), &wsl_git).is_err());

        let docker = RunRequest {
            apps: Vec::new(),
            wsl: vec![WslAction::DockerEngine],
        };
        assert!(matches!(
            plan(&catalog, &windows(), Some(&ubuntu(false)), &docker),
            Err(Error::Wsl(_))
        ));
        assert!(plan(&catalog, &windows(), Some(&ubuntu(true)), &docker).is_ok());
    }

    #[test]
    fn wsl_is_rejected_outside_windows_and_bad_distro_names_too() {
        let catalog = Catalog::parse(CATALOG).unwrap();
        let install = |distro: &str| RunRequest {
            apps: Vec::new(),
            wsl: vec![WslAction::InstallDistro {
                distro: distro.into(),
            }],
        };
        assert!(
            plan(
                &catalog,
                &platform(Os::Linux, &[]),
                None,
                &install("Ubuntu")
            )
            .is_err()
        );
        assert!(plan(&catalog, &windows(), None, &install("Ubuntu; rm")).is_err());
        let tasks = plan(&catalog, &windows(), None, &install("Ubuntu")).unwrap();
        assert_eq!(task_ids(&tasks), ["wsl-distro-Ubuntu"]);
    }

    #[test]
    fn dry_run_emits_commands_without_executing() {
        let tasks = vec![Task {
            id: "x".into(),
            label: "X".into(),
            action: InstallAction::Command(Cmd::new("program-that-does-not-exist").arg("--flag")),
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
                action: InstallAction::Command(Cmd::new("cargo").arg("--version")),
                kind: TaskKind::Setup,
            },
            Task {
                id: "missing".into(),
                label: "missing".into(),
                action: InstallAction::Command(Cmd::new("program-that-does-not-exist")),
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

//! Builds the task queue and runs it, streaming output. Every install goes through here,
//! and every run can be a dry run (commands shown, nothing executed).

use std::collections::HashSet;
use std::io::{Read, Write};
use std::sync::{Arc, mpsc};
use std::thread;

use serde::{Deserialize, Serialize};

use crate::catalog::{App, Catalog, Target};
use crate::debloat::{self, DebloatCatalog};
use crate::error::{Error, Result};
use crate::exec::Cmd;
use crate::platform::{Os, PlatformInfo};
use crate::pm::{self, InstallAction, InstallOutcome, PmId, apt, github};
use crate::resolve::{Resolution, resolve, resolve_wsl};
use crate::root::RootSession;
use crate::tweaks::{self, TweakCatalog, TweakJob, TweakState};
use crate::wsl::{self, DistroDetails, WslAction};

#[derive(Debug, Clone)]
pub struct Task {
    pub id: String,
    pub label: String,
    pub action: InstallAction,
    pub kind: TaskKind,
    /// A task that must have finished first; if it failed (or was skipped), this one is skipped.
    pub needs: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub enum TaskKind {
    Setup,
    Install(PmId),
    /// Debloat: uninstalling something.
    Remove,
}

/// Mirrored in `src/lib/types.ts` (`TaskStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskStatus {
    Succeeded,
    /// A setup step or tweak (not an app install) that finished.
    Done,
    /// A debloat item that was uninstalled.
    Removed,
    /// Not run because a task it needs didn't finish.
    Skipped,
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
    #[serde(default)]
    pub tweaks: Vec<TweakRequest>,
    /// Debloat item IDs to remove.
    #[serde(default)]
    pub debloat: Vec<String>,
    /// Linux: install Flatpak with the distro's package manager and add Flathub.
    #[serde(default)]
    pub setup_flatpak: bool,
    #[serde(default)]
    pub postinstall: Vec<crate::postinstall::PostInstallRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TweakRequest {
    pub id: String,
    /// Restore the values saved when the tweak was applied.
    #[serde(default)]
    pub revert: bool,
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
        needs: None,
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
                let mut task = setup_task(step.id, step.label, step.cmd);
                if step.root
                    && let InstallAction::Command(cmd) = task.action
                {
                    task.action = InstallAction::Root(cmd);
                }
                host_setups.push(task);
            }
        }
        host_installs.push(Task {
            id: app.id.clone(),
            label: app.name.clone(),
            action,
            kind: TaskKind::Install(manager),
            needs: None,
        });
    }

    let mut tasks = plan_flatpak_setup(platform, request.setup_flatpak)?;
    tasks.extend(host_setups);
    tasks.extend(host_installs);
    tasks.extend(plan_wsl(platform, distro, &wsl_apps, &request.wsl)?);
    // Setup steps shared by several parts (e.g. the apt update) run once, where they first appear.
    let mut ids = HashSet::new();
    tasks.retain(|task| ids.insert(task.id.clone()));
    Ok(tasks)
}

/// Linux without Flatpak: install it from the distro and add Flathub for the user. Apps from Flathub
/// show up after this run, once In8 detects Flatpak again.
fn plan_flatpak_setup(platform: &PlatformInfo, wanted: bool) -> Result<Vec<Task>> {
    if !wanted || platform.has(PmId::Flatpak) {
        return Ok(Vec::new());
    }
    let manager = platform
        .native_manager()
        .filter(|_| platform.os == Os::Linux)
        .ok_or_else(|| Error::Unavailable("Flatpak".into()))?;
    let mut tasks = Vec::new();
    let install = match manager {
        PmId::Apt => {
            tasks.push(Task {
                id: "setup-apt-update".into(),
                label: "Update package lists".into(),
                action: InstallAction::Root(apt::native_update_cmd()),
                kind: TaskKind::Setup,
                needs: None,
            });
            apt::native_install_cmd(&["flatpak"])
        }
        PmId::Dnf => pm::dnf::install_cmd(&["flatpak"]),
        _ => pm::pacman::install_cmd(&["flatpak"]),
    };
    tasks.push(Task {
        id: "setup-flatpak".into(),
        label: "Install Flatpak".into(),
        action: InstallAction::Root(install),
        kind: TaskKind::Install(manager),
        needs: None,
    });
    let mut flathub = setup_task(
        format!("setup-flatpak-{}", pm::flatpak::FLATHUB_NAME),
        "Add the Flathub repository",
        pm::flatpak::add_remote_cmd(pm::flatpak::FLATHUB_NAME, pm::flatpak::FLATHUB_REPO),
    );
    // The remote is added with the flatpak command the previous task installs.
    flathub.needs = Some("setup-flatpak".into());
    tasks.push(flathub);
    Ok(tasks)
}

/// Catalogs and saved state for debloat and tweaks.
pub struct SystemContext<'a> {
    pub tweaks: &'a TweakCatalog,
    pub debloat: &'a DebloatCatalog,
    pub state: &'a Arc<TweakState>,
}

/// Debloat tasks (run before installs) and tweak tasks (run after everything else).
pub fn plan_system(
    context: &SystemContext,
    platform: &PlatformInfo,
    request: &RunRequest,
) -> Result<(Vec<Task>, Vec<Task>)> {
    let mut removals = Vec::new();
    let mut seen = HashSet::new();
    for id in &request.debloat {
        if !seen.insert(id.as_str()) {
            continue;
        }
        let item = context
            .debloat
            .get(id)
            .ok_or_else(|| Error::UnknownItem(id.clone()))?;
        if platform.os != Os::Windows {
            return Err(Error::Unavailable(item.name.clone()));
        }
        removals.push(Task {
            id: format!("debloat-{id}"),
            label: format!("Remove {}", item.name),
            action: InstallAction::Command(debloat::remove_cmd(item)),
            kind: TaskKind::Remove,
            needs: None,
        });
    }

    let mut tweak_tasks = Vec::new();
    let mut seen = HashSet::new();
    for wanted in &request.tweaks {
        if !seen.insert(wanted.id.as_str()) {
            continue;
        }
        let tweak = context
            .tweaks
            .get(&wanted.id)
            .ok_or_else(|| Error::UnknownItem(wanted.id.clone()))?;
        if !tweaks::supported(tweak, platform) {
            return Err(Error::Unavailable(tweak.name.clone()));
        }
        tweak_tasks.push(Task {
            id: format!("tweak-{}", tweak.id),
            label: if wanted.revert {
                format!("Revert: {}", tweak.name)
            } else {
                tweak.name.clone()
            },
            action: InstallAction::Tweak(Box::new(TweakJob {
                tweak: tweak.clone(),
                revert: wanted.revert,
                state: Arc::clone(context.state),
            })),
            kind: TaskKind::Setup,
            needs: None,
        });
    }
    Ok((removals, tweak_tasks))
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
                needs: None,
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
                needs: None,
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
            needs: None,
        });
    }
    Ok(tasks)
}

/// Root on native Linux: the pkexec helper starts on the first task that needs it and lasts the run.
enum RootState {
    NotStarted,
    Running(RootSession),
    /// The password was refused or pkexec is missing; later root tasks fail without asking again.
    Failed(String),
}

pub fn run(tasks: &[Task], dry_run: bool, mut emit: impl FnMut(RunEvent)) -> RunSummary {
    let (mut succeeded, mut failed) = (0, 0);
    let mut root = RootState::NotStarted;
    // Tasks that didn't finish, so the ones that need them are skipped.
    let mut not_done: HashSet<&str> = HashSet::new();
    for task in tasks {
        emit(RunEvent::TaskStarted {
            task_id: task.id.clone(),
            label: task.label.clone(),
            command: task.action.display(),
        });
        let blocked_by = task
            .needs
            .as_deref()
            .filter(|needed| not_done.contains(needed))
            .map(|needed| {
                tasks
                    .iter()
                    .find(|t| t.id == needed)
                    .map_or(needed, |t| t.label.as_str())
            });
        let (status, exit_code) = if let Some(label) = blocked_by {
            emit(RunEvent::Output {
                task_id: task.id.clone(),
                line: format!("Skipped: \"{label}\" didn't finish."),
            });
            (TaskStatus::Skipped, None)
        } else if dry_run {
            (TaskStatus::Simulated, None)
        } else {
            execute(task, &mut root, &mut emit)
        };
        if matches!(status, TaskStatus::Failed | TaskStatus::Skipped) {
            not_done.insert(task.id.as_str());
        }
        if matches!(status, TaskStatus::Failed | TaskStatus::Skipped) {
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

fn execute(
    task: &Task,
    root: &mut RootState,
    emit: &mut impl FnMut(RunEvent),
) -> (TaskStatus, Option<i32>) {
    let output = |line: String| RunEvent::Output {
        task_id: task.id.clone(),
        line,
    };
    let code = match &task.action {
        InstallAction::Command(cmd) => run_process(&task.id, cmd, emit),
        InstallAction::Tweak(job) => {
            let mut log = |line: String| emit(output(line));
            let result = if job.revert {
                tweaks::revert(&job.tweak, &job.state, &mut log)
            } else {
                tweaks::apply(&job.tweak, &job.state, &mut log)
            };
            return match result {
                Ok(()) => (TaskStatus::Done, Some(0)),
                Err(message) => {
                    emit(output(message));
                    (TaskStatus::Failed, None)
                }
            };
        }
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
        InstallAction::VscodeWslExtension { distro, extension } => {
            match wsl::code_shim_in(distro) {
                Some(shim) => {
                    let cmd = wsl::in_home(
                        distro,
                        Cmd::new(shim).args(["--install-extension", extension.as_str()]),
                    );
                    run_process(&task.id, &cmd, emit)
                }
                None => {
                    emit(output(
                        "VS Code wasn't found on Windows. Install it first, then try again.".into(),
                    ));
                    return (TaskStatus::Failed, None);
                }
            }
        }
        InstallAction::Root(cmd) => {
            if matches!(root, RootState::NotStarted) {
                emit(output("Asking for the administrator password…".into()));
                *root = match RootSession::start() {
                    Ok(session) => RootState::Running(session),
                    Err(message) => RootState::Failed(message),
                };
            }
            match root {
                RootState::Running(session) => {
                    let mut log = |line: String| emit(output(line));
                    session.run(cmd, &mut log)
                }
                RootState::Failed(message) => {
                    emit(output(message.clone()));
                    return (TaskStatus::Failed, None);
                }
                RootState::NotStarted => return (TaskStatus::Failed, None),
            }
        }
        InstallAction::HostTool(cmd) => {
            let mut cmd = cmd.clone();
            cmd.program = crate::exec::locate(&cmd.program);
            run_process(&task.id, &cmd, emit)
        }
        InstallAction::CreateDir(path) => {
            let existed = path.is_dir();
            return match std::fs::create_dir_all(path) {
                Ok(()) => {
                    let what = if existed { "Already there" } else { "Created" };
                    emit(output(format!("{what}: {}", path.display())));
                    (TaskStatus::Done, Some(0))
                }
                Err(error) => {
                    emit(output(format!(
                        "Couldn't create {}: {error}",
                        path.display()
                    )));
                    (TaskStatus::Failed, None)
                }
            };
        }
        InstallAction::SshKey(job) => {
            let mut log = |line: String| emit(output(line));
            crate::postinstall::run_ssh_key(job, &mut log)
        }
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
        TaskKind::Remove if code == Some(0) => return TaskStatus::Removed,
        TaskKind::Remove => InstallOutcome::Failed,
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
            prefer_native: false,
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
            ..Default::default()
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
            ..Default::default()
        };
        assert!(matches!(
            plan(&catalog, &windows(), Some(&ubuntu(false)), &docker),
            Err(Error::Wsl(_))
        ));
        assert!(plan(&catalog, &windows(), Some(&ubuntu(true)), &docker).is_ok());
    }

    #[test]
    fn tasks_whose_prerequisite_failed_are_skipped() {
        let task = |id: &str, needs: Option<&str>| Task {
            id: id.into(),
            label: id.into(),
            action: InstallAction::Command(Cmd::new("in8-no-such-program")),
            kind: TaskKind::Setup,
            needs: needs.map(str::to_string),
        };
        let tasks = [task("first", None), task("second", Some("first"))];
        let mut statuses = Vec::new();
        let summary = run(&tasks, false, |event| {
            if let RunEvent::TaskFinished { status, .. } = event {
                statuses.push(status);
            }
        });
        assert_eq!(statuses, [TaskStatus::Failed, TaskStatus::Skipped]);
        assert_eq!(summary.failed, 2);
    }

    #[test]
    fn native_linux_installs_run_as_root_after_one_package_list_update() {
        let catalog = Catalog::load_embedded().unwrap();
        let ubuntu = platform(Os::Linux, &[PmId::Apt]);
        let tasks = plan(
            &catalog,
            &ubuntu,
            None,
            &RunRequest {
                setup_flatpak: true,
                ..apps(&[("git", Target::Host), ("python", Target::Host)])
            },
        )
        .unwrap();
        assert_eq!(
            task_ids(&tasks),
            [
                "setup-apt-update",
                "setup-flatpak",
                "setup-flatpak-flathub",
                "git",
                "python"
            ]
        );
        assert!(matches!(tasks[0].action, InstallAction::Root(_)));
        assert!(matches!(tasks[2].action, InstallAction::Command(_)));
        let InstallAction::Root(cmd) = &tasks[3].action else {
            panic!("git should install as root")
        };
        assert_eq!(cmd.program, "apt-get");
        assert_eq!(cmd.args, ["install", "-y", "git"]);

        // With Flatpak already there, asking to set it up does nothing.
        let with_flatpak = platform(Os::Linux, &[PmId::Flatpak, PmId::Apt]);
        let request = RunRequest {
            setup_flatpak: true,
            ..Default::default()
        };
        assert!(
            plan(&catalog, &with_flatpak, None, &request)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn plans_debloat_and_tweaks_once_each() {
        let tweaks = TweakCatalog::load_embedded().unwrap();
        let debloat = DebloatCatalog::load_embedded().unwrap();
        let state = Arc::new(TweakState::new(
            &std::env::temp_dir().join("in8-plan-system"),
        ));
        let context = SystemContext {
            tweaks: &tweaks,
            debloat: &debloat,
            state: &state,
        };
        let tweak = |id: &str, revert: bool| TweakRequest {
            id: id.into(),
            revert,
        };
        let request = RunRequest {
            tweaks: vec![tweak("dark-mode", false), tweak("dark-mode", true)],
            debloat: vec!["clipchamp".into(), "clipchamp".into()],
            ..Default::default()
        };
        let (removals, tweak_tasks) = plan_system(&context, &windows(), &request).unwrap();
        assert_eq!(task_ids(&removals), ["debloat-clipchamp"]);
        assert_eq!(task_ids(&tweak_tasks), ["tweak-dark-mode"]);
        assert!(plan_system(&context, &platform(Os::Linux, &[]), &request).is_err());

        let unknown = RunRequest {
            debloat: vec!["nope".into()],
            ..Default::default()
        };
        assert!(matches!(
            plan_system(&context, &windows(), &unknown),
            Err(Error::UnknownItem(_))
        ));
    }

    #[test]
    fn wsl_is_rejected_outside_windows_and_bad_distro_names_too() {
        let catalog = Catalog::parse(CATALOG).unwrap();
        let install = |distro: &str| RunRequest {
            apps: Vec::new(),
            wsl: vec![WslAction::InstallDistro {
                distro: distro.into(),
            }],
            ..Default::default()
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
            needs: None,
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
                needs: None,
            },
            Task {
                id: "missing".into(),
                label: "missing".into(),
                action: InstallAction::Command(Cmd::new("program-that-does-not-exist")),
                kind: TaskKind::Setup,
                needs: None,
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

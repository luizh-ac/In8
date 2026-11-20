//! Package managers. Each one only builds commands and interprets results;
//! the runner is the only place that executes installs.

pub mod apt;
pub mod dnf;
pub mod flatpak;
pub mod github;
pub mod pacman;
pub mod winget;

use serde::{Deserialize, Serialize};

use crate::catalog::{App, GithubPackage, WingetPackage};
use crate::error::Result;
use crate::exec::Cmd;
use crate::platform::Os;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PmId {
    Winget,
    /// Installers downloaded from GitHub releases (Windows).
    Github,
    Flatpak,
    /// Debian/Ubuntu: inside WSL distros (as root through wsl.exe) and on native Linux (through pkexec).
    Apt,
    /// Fedora and other RPM distros (native Linux, through pkexec).
    Dnf,
    /// Arch and derivatives (native Linux, through pkexec).
    Pacman,
}

impl PmId {
    /// apt, dnf and pacman: the distro's own package managers, which need root.
    pub fn is_native(self) -> bool {
        matches!(self, Self::Apt | Self::Dnf | Self::Pacman)
    }
}

/// The distro's package manager on native Linux: the first one found on the PATH.
pub fn detect_native() -> Option<PmId> {
    [
        ("apt-get", PmId::Apt),
        ("dnf", PmId::Dnf),
        ("pacman", PmId::Pacman),
    ]
    .into_iter()
    .find(|(program, _)| crate::exec::is_on_path(program))
    .map(|(_, manager)| manager)
}

/// What a finished install command means, according to the package manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallOutcome {
    Installed,
    AlreadyInstalled,
    Failed,
}

/// How an app gets installed.
#[derive(Debug, Clone)]
pub enum InstallAction {
    Command(Cmd),
    GithubRelease(GithubPackage),
    /// `code --install-extension <id>`; VS Code is located when the task runs.
    VscodeExtension(String),
    /// Applies or reverts a tweak in-process (registry, power plan, gsettings).
    Tweak(Box<crate::tweaks::TweakJob>),
    /// A program that may have been installed earlier in the same run (e.g. Git): it's looked up
    /// when the task runs, not when it's planned.
    HostTool(Cmd),
    /// Creates a folder (and its parents) on this machine.
    CreateDir(std::path::PathBuf),
    /// `ssh-keygen` in a terminal window, unless the key already exists.
    SshKey(crate::postinstall::SshKeyJob),
    /// A command that needs root on native Linux; it runs through the pkexec helper (one password per run).
    Root(Cmd),
    /// `code --install-extension` run inside WSL, so the extension lands on the WSL side.
    VscodeWslExtension {
        distro: String,
        extension: String,
    },
}

impl InstallAction {
    pub fn display(&self) -> String {
        match self {
            Self::Tweak(job) => job.tweak.describe(job.revert),
            Self::Command(cmd) => cmd.display(),
            Self::GithubRelease(package) => github::describe(package),
            Self::VscodeExtension(extension) => format!("code --install-extension {extension}"),
            Self::HostTool(cmd) => cmd.display(),
            Self::Root(cmd) => format!("(as administrator) {}", cmd.display()),
            Self::CreateDir(path) => crate::postinstall::describe_dir(path),
            Self::SshKey(job) => crate::postinstall::describe_ssh(job),
            Self::VscodeWslExtension { distro, extension } => {
                format!("code --install-extension {extension} (inside {distro})")
            }
        }
    }
}

/// A one-time step needed before installing (e.g. adding a Flatpak remote).
/// Steps with the same `id` run only once per queue.
pub struct SetupStep {
    pub id: String,
    pub label: String,
    pub cmd: Cmd,
    /// Runs as root (through the pkexec helper).
    pub root: bool,
}

pub fn detect_available(os: Os) -> Vec<PmId> {
    match os {
        Os::Windows => {
            let mut managers = Vec::new();
            if winget::is_available() {
                managers.push(PmId::Winget);
            }
            // Only needs msiexec, which every Windows has.
            managers.push(PmId::Github);
            managers
        }
        Os::Linux => flatpak::is_available()
            .then_some(PmId::Flatpak)
            .into_iter()
            .chain(detect_native())
            .collect(),
        Os::Other => Vec::new(),
    }
}

pub fn install_action(manager: PmId, app: &App) -> Option<InstallAction> {
    match manager {
        PmId::Winget => app
            .packages
            .winget
            .as_ref()
            .map(|package| InstallAction::Command(winget::install_cmd(package))),
        PmId::Github => app
            .packages
            .github
            .clone()
            .map(InstallAction::GithubRelease),
        PmId::Flatpak => app
            .packages
            .flatpak
            .as_ref()
            .map(|package| InstallAction::Command(flatpak::install_cmd(package))),
        // Native Linux only: WSL installs are planned by the runner's WSL step.
        PmId::Apt => app
            .packages
            .apt
            .as_deref()
            .map(|package| InstallAction::Root(apt::native_install_cmd(&[package]))),
        PmId::Dnf => app
            .packages
            .dnf
            .as_deref()
            .map(|package| InstallAction::Root(dnf::install_cmd(&[package]))),
        PmId::Pacman => app
            .packages
            .pacman
            .as_deref()
            .map(|package| InstallAction::Root(pacman::install_cmd(&[package]))),
    }
}

pub fn setup_steps(manager: PmId, app: &App) -> Vec<SetupStep> {
    match (manager, &app.packages.flatpak) {
        (PmId::Flatpak, Some(package)) => {
            let (name, url) = flatpak::remote_of(package);
            vec![SetupStep {
                id: format!("setup-flatpak-{name}"),
                label: format!("Add the {name} Flatpak repository"),
                cmd: flatpak::add_remote_cmd(name, url),
                root: false,
            }]
        }
        // apt needs fresh package lists; dnf refreshes on its own and pacman syncs with -Syu.
        (PmId::Apt, _) => vec![SetupStep {
            id: "setup-apt-update".into(),
            label: "Update package lists".into(),
            cmd: apt::native_update_cmd(),
            root: true,
        }],
        _ => Vec::new(),
    }
}

pub fn interpret_exit(manager: PmId, code: Option<i32>) -> InstallOutcome {
    match manager {
        PmId::Winget => winget::interpret_exit(code),
        PmId::Github => github::interpret_exit(code),
        PmId::Flatpak => flatpak::interpret_exit(code),
        PmId::Apt | PmId::Dnf | PmId::Pacman => apt::interpret_exit(code),
    }
}

/// Package IDs installed through this manager, considering at least the given apps' packages.
pub fn installed_packages(manager: PmId, apps: &[&App]) -> Result<Vec<String>> {
    match manager {
        PmId::Winget => {
            let packages: Vec<&WingetPackage> = apps
                .iter()
                .filter_map(|app| app.packages.winget.as_ref())
                .collect();
            winget::installed_among(&packages)
        }
        PmId::Github => Ok(apps
            .iter()
            .filter_map(|app| app.packages.github.as_ref())
            .filter(|package| github::is_installed(package))
            .map(|package| package.repo.clone())
            .collect()),
        PmId::Flatpak => flatpak::list_installed(),
        PmId::Apt | PmId::Dnf | PmId::Pacman => {
            let packages: Vec<&str> = apps
                .iter()
                .filter_map(|app| app.packages.native(manager))
                .collect();
            if packages.is_empty() {
                return Ok(Vec::new());
            }
            let query = match manager {
                PmId::Apt => apt::query_cmd(&packages),
                PmId::Dnf => dnf::query_cmd(&packages),
                _ => pacman::query_cmd(&packages),
            };
            let output = crate::exec::capture_stdout(&query).unwrap_or_default();
            Ok(match manager {
                PmId::Apt => apt::parse_query(&output),
                PmId::Dnf => dnf::parse_query(&output, &packages),
                _ => pacman::parse_query(&output, &packages),
            })
        }
    }
}

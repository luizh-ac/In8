//! Package managers. Each one only builds commands and interprets results;
//! the runner is the only place that executes installs.

pub mod apt;
pub mod flatpak;
pub mod github;
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
    /// Inside WSL distros for now; native Linux comes later.
    Apt,
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
}

impl InstallAction {
    pub fn display(&self) -> String {
        match self {
            Self::Tweak(job) => job.tweak.describe(job.revert),
            Self::Command(cmd) => cmd.display(),
            Self::GithubRelease(package) => github::describe(package),
            Self::VscodeExtension(extension) => format!("code --install-extension {extension}"),
        }
    }
}

/// A one-time step needed before installing (e.g. adding a Flatpak remote).
/// Steps with the same `id` run only once per queue.
pub struct SetupStep {
    pub id: String,
    pub label: String,
    pub cmd: Cmd,
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
        Os::Linux if flatpak::is_available() => vec![PmId::Flatpak],
        Os::Linux | Os::Other => Vec::new(),
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
        PmId::Apt => app
            .packages
            .apt
            .as_deref()
            .map(|package| InstallAction::Command(apt::install_cmd(&[package]))),
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
            }]
        }
        _ => Vec::new(),
    }
}

pub fn interpret_exit(manager: PmId, code: Option<i32>) -> InstallOutcome {
    match manager {
        PmId::Winget => winget::interpret_exit(code),
        PmId::Github => github::interpret_exit(code),
        PmId::Flatpak => flatpak::interpret_exit(code),
        PmId::Apt => apt::interpret_exit(code),
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
        PmId::Apt => {
            let packages: Vec<&str> = apps
                .iter()
                .filter_map(|app| app.packages.apt.as_deref())
                .collect();
            Ok(crate::exec::capture_stdout(&apt::query_cmd(&packages))
                .map(|output| apt::parse_query(&output))
                .unwrap_or_default())
        }
    }
}

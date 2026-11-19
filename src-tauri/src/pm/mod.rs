//! Package managers. Each one only builds commands and interprets results;
//! the runner is the only place that executes installs.

pub mod flatpak;
pub mod winget;

use serde::{Deserialize, Serialize};

use crate::catalog::{App, WingetPackage};
use crate::error::Result;
use crate::exec::Cmd;
use crate::platform::Os;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PmId {
    Winget,
    Flatpak,
}

impl PmId {
    pub fn as_str(self) -> &'static str {
        match self {
            PmId::Winget => "winget",
            PmId::Flatpak => "flatpak",
        }
    }
}

/// What a finished install command means, according to the package manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallOutcome {
    Installed,
    AlreadyInstalled,
    Failed,
}

/// A one-time step a manager needs before installing (e.g. adding Flathub).
pub struct SetupStep {
    pub label: &'static str,
    pub cmd: Cmd,
}

pub fn detect_available(os: Os) -> Vec<PmId> {
    let candidates: &[PmId] = match os {
        Os::Windows => &[PmId::Winget],
        Os::Linux => &[PmId::Flatpak],
        Os::Other => &[],
    };
    candidates
        .iter()
        .copied()
        .filter(|&manager| match manager {
            PmId::Winget => winget::is_available(),
            PmId::Flatpak => flatpak::is_available(),
        })
        .collect()
}

pub fn install_cmd(manager: PmId, app: &App) -> Option<Cmd> {
    match manager {
        PmId::Winget => app.packages.winget.as_ref().map(winget::install_cmd),
        PmId::Flatpak => app.packages.flatpak.as_deref().map(flatpak::install_cmd),
    }
}

pub fn setup_step(manager: PmId) -> Option<SetupStep> {
    match manager {
        PmId::Winget => None,
        PmId::Flatpak => Some(SetupStep {
            label: "Add the Flathub repository",
            cmd: flatpak::add_flathub_cmd(),
        }),
    }
}

pub fn interpret_exit(manager: PmId, code: Option<i32>) -> InstallOutcome {
    match manager {
        PmId::Winget => winget::interpret_exit(code),
        PmId::Flatpak => flatpak::interpret_exit(code),
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
        PmId::Flatpak => flatpak::list_installed(),
    }
}

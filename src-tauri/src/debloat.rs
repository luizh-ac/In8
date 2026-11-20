//! Debloat: removing apps that come with Windows. Only for the current user; apps can come back
//! from the Microsoft Store, but In8 can't undo a removal by itself.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::catalog::WingetPackage;
use crate::error::{Error, Result};
use crate::exec::{self, Cmd};
use crate::platform::{Os, PlatformInfo};
use crate::pm::winget;

const EMBEDDED_DEBLOAT: &str = include_str!("../../catalog/debloat.json");

/// Removes every package named in `IN8_APPX` (`;`-separated). The names travel in an environment
/// variable, so the script itself is constant and nothing from data becomes PowerShell code.
const REMOVE_APPX_SCRIPT: &str = "$ErrorActionPreference = 'Stop'; $ProgressPreference = 'SilentlyContinue'; \
foreach ($name in $env:IN8_APPX.Split(';')) { \
foreach ($package in @(Get-AppxPackage -Name $name)) { \
Write-Output \"Removing $($package.PackageFullName)\"; Remove-AppxPackage -Package $package.PackageFullName } }";

const LIST_APPX_SCRIPT: &str =
    "$ProgressPreference = 'SilentlyContinue'; Get-AppxPackage | ForEach-Object { $_.Name }";

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DebloatCatalog {
    pub version: u32,
    pub items: Vec<DebloatItem>,
}

/// Mirrored in `src/lib/types.ts` (`DebloatItem`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DebloatItem {
    pub id: String,
    pub name: String,
    pub description: String,
    pub level: Level,
    /// Appx package names (`Get-AppxPackage -Name`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub appx: Vec<String>,
    /// A winget package to uninstall (for apps that aren't Appx, like OneDrive).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub winget: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// Nothing depends on it.
    Safe,
    /// Can break something someone uses; the UI asks before running.
    Advanced,
}

/// Mirrored in `src/lib/types.ts` (`DebloatInfo`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DebloatInfo {
    #[serde(flatten)]
    pub item: DebloatItem,
    pub installed: bool,
}

fn is_appx_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
}

impl DebloatCatalog {
    pub fn load_embedded() -> Result<Self> {
        Self::parse(EMBEDDED_DEBLOAT)
    }

    pub fn parse(json: &str) -> Result<Self> {
        let catalog: DebloatCatalog =
            serde_json::from_str(json).map_err(|e| Error::Catalog(format!("debloat: {e}")))?;
        let mut seen = HashSet::new();
        for item in &catalog.items {
            let invalid =
                |reason: &str| Error::Catalog(format!("debloat \"{}\": {reason}", item.id));
            if item.id.is_empty()
                || !item
                    .id
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            {
                return Err(invalid("bad id"));
            }
            if !seen.insert(item.id.as_str()) {
                return Err(invalid("duplicate id"));
            }
            if item.appx.is_empty() == item.winget.is_none() {
                return Err(invalid("needs either appx or winget"));
            }
            if !item.appx.iter().all(|name| is_appx_name(name)) {
                return Err(invalid(
                    "appx names use letters, digits, dots, dashes and underscores",
                ));
            }
        }
        Ok(catalog)
    }

    pub fn get(&self, id: &str) -> Option<&DebloatItem> {
        self.items.iter().find(|item| item.id == id)
    }
}

/// Every item with whether it's installed for this user. Empty outside Windows.
pub fn list(catalog: &DebloatCatalog, platform: &PlatformInfo) -> Result<Vec<DebloatInfo>> {
    if platform.os != Os::Windows {
        return Ok(Vec::new());
    }
    let appx: HashSet<String> = exec::capture(&powershell(LIST_APPX_SCRIPT))
        .map(|output| output.lines().map(|l| l.trim().to_lowercase()).collect())
        .unwrap_or_default();
    let winget_packages: Vec<WingetPackage> = catalog
        .items
        .iter()
        .filter_map(|item| item.winget.clone().map(WingetPackage::Id))
        .collect();
    let winget_installed: HashSet<String> = if winget_packages.is_empty() {
        HashSet::new()
    } else {
        winget::installed_among(&winget_packages.iter().collect::<Vec<_>>())?
            .into_iter()
            .map(|id| id.to_lowercase())
            .collect()
    };
    Ok(catalog
        .items
        .iter()
        .map(|item| DebloatInfo {
            installed: item
                .appx
                .iter()
                .any(|name| appx.contains(&name.to_lowercase()))
                || item
                    .winget
                    .as_ref()
                    .is_some_and(|id| winget_installed.contains(&id.to_lowercase())),
            item: item.clone(),
        })
        .collect())
}

fn powershell(script: &str) -> Cmd {
    Cmd::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", script])
}

/// The command that removes an item for the current user.
pub fn remove_cmd(item: &DebloatItem) -> Cmd {
    match &item.winget {
        Some(id) => Cmd::new("winget").args([
            "uninstall",
            "--id",
            id.as_str(),
            "--exact",
            "--silent",
            "--accept-source-agreements",
            "--disable-interactivity",
        ]),
        None => powershell(REMOVE_APPX_SCRIPT).env("IN8_APPX", item.appx.join(";")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_debloat_is_valid() {
        let catalog =
            DebloatCatalog::load_embedded().expect("embedded debloat list should be valid");
        assert_eq!(catalog.get("xbox").unwrap().level, Level::Advanced);
        assert_eq!(catalog.get("clipchamp").unwrap().level, Level::Safe);
    }

    #[test]
    fn appx_names_go_through_the_environment() {
        let catalog = DebloatCatalog::load_embedded().unwrap();
        let cmd = remove_cmd(catalog.get("outlook-mail").unwrap());
        assert_eq!(
            cmd.args.last().map(String::as_str),
            Some(REMOVE_APPX_SCRIPT)
        );
        assert_eq!(
            cmd.env,
            [(
                "IN8_APPX".to_string(),
                "Microsoft.OutlookForWindows;microsoft.windowscommunicationsapps".to_string()
            )]
        );
        let onedrive = remove_cmd(catalog.get("onedrive").unwrap());
        assert_eq!(onedrive.program, "winget");
    }

    /// Reads this machine (nothing changes): which debloat items are installed and which tweaks are applied.
    /// Run with `cargo test real_system_state -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_system_state() {
        let platform = PlatformInfo::detect();
        for info in list(&DebloatCatalog::load_embedded().unwrap(), &platform).unwrap() {
            println!("debloat {:<14} installed={}", info.item.id, info.installed);
        }
        let state = crate::tweaks::TweakState::new(&std::env::temp_dir().join("in8-real-state"));
        let catalog = crate::tweaks::TweakCatalog::load_embedded().unwrap();
        for info in crate::tweaks::list(&catalog, &platform, &state) {
            println!("tweak {:<26} applied={}", info.tweak.id, info.applied);
        }
    }

    #[test]
    fn rejects_unsafe_appx_names() {
        let json = r#"{ "version": 1, "items": [ { "id": "x", "name": "X", "description": "",
            "level": "safe", "appx": ["Microsoft.*"] } ] }"#;
        assert!(DebloatCatalog::parse(json).is_err());
        let both = r#"{ "version": 1, "items": [ { "id": "x", "name": "X", "description": "",
            "level": "safe" } ] }"#;
        assert!(DebloatCatalog::parse(both).is_err());
    }
}

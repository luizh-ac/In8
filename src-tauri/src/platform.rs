//! Detects the operating system, the Linux distro and the available package managers.

use serde::Serialize;

use crate::pm::{self, PmId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Windows,
    Linux,
    Other,
}

impl Os {
    pub fn current() -> Self {
        match std::env::consts::OS {
            "windows" => Os::Windows,
            "linux" => Os::Linux,
            _ => Os::Other,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Distro {
    pub id: String,
    pub name: String,
    pub id_like: Vec<String>,
}

/// Mirrored in `src/lib/types.ts` (`PlatformInfo`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformInfo {
    pub os: Os,
    pub arch: &'static str,
    pub version: &'static str,
    pub distro: Option<Distro>,
    pub managers: Vec<PmId>,
    /// Linux: try the distro's packages before Flatpak (setting "Prefer distro packages").
    pub prefer_native: bool,
}

impl PlatformInfo {
    #[cfg(test)]
    pub fn detect() -> Self {
        Self::detect_with(false)
    }

    pub fn detect_with(prefer_native: bool) -> Self {
        let os = Os::current();
        let distro = match os {
            Os::Linux => std::fs::read_to_string("/etc/os-release")
                .ok()
                .map(|content| parse_os_release(&content)),
            _ => None,
        };
        Self {
            os,
            arch: std::env::consts::ARCH,
            version: env!("CARGO_PKG_VERSION"),
            distro,
            managers: pm::detect_available(os),
            prefer_native,
        }
    }

    pub fn has(&self, manager: PmId) -> bool {
        self.managers.contains(&manager)
    }

    /// The distro's own package manager (apt, dnf or pacman) on native Linux.
    pub fn native_manager(&self) -> Option<PmId> {
        self.managers
            .iter()
            .copied()
            .find(|manager| manager.is_native())
    }
}

pub fn parse_os_release(content: &str) -> Distro {
    let (mut id, mut name, mut pretty_name) = (String::new(), String::new(), String::new());
    let mut id_like = Vec::new();
    for line in content.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"').trim_matches('\'');
        match key.trim() {
            "ID" => id = value.to_string(),
            "NAME" => name = value.to_string(),
            "PRETTY_NAME" => pretty_name = value.to_string(),
            "ID_LIKE" => id_like = value.split_whitespace().map(str::to_string).collect(),
            _ => {}
        }
    }
    Distro {
        id,
        name: if pretty_name.is_empty() {
            name
        } else {
            pretty_name
        },
        id_like,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ubuntu_os_release() {
        let content = "PRETTY_NAME=\"Ubuntu 26.04 LTS\"\nNAME=\"Ubuntu\"\nID=ubuntu\nID_LIKE=debian\n# comment\n";
        let distro = parse_os_release(content);
        assert_eq!(distro.id, "ubuntu");
        assert_eq!(distro.name, "Ubuntu 26.04 LTS");
        assert_eq!(distro.id_like, ["debian"]);
    }

    #[test]
    fn falls_back_to_name_and_splits_id_like() {
        let distro = parse_os_release("NAME='Nobara'\nID=nobara\nID_LIKE=\"rhel centos fedora\"");
        assert_eq!(distro.name, "Nobara");
        assert_eq!(distro.id_like, ["rhel", "centos", "fedora"]);
    }
}

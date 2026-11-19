//! Decides which package manager installs each app on the current system.

use serde::Serialize;

use crate::catalog::App;
use crate::platform::{Os, PlatformInfo};
use crate::pm::PmId;

/// Mirrored in `src/lib/types.ts` (`Resolution`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Resolution {
    Available { manager: PmId, package: String },
    Unavailable { reason: String },
}

fn unavailable(reason: &str) -> Resolution {
    Resolution::Unavailable {
        reason: reason.to_string(),
    }
}

pub fn resolve(app: &App, platform: &PlatformInfo) -> Resolution {
    match platform.os {
        Os::Windows => match &app.packages.winget {
            None => unavailable("Not available for Windows"),
            Some(_) if !platform.has(PmId::Winget) => unavailable("winget wasn't found"),
            Some(package) => Resolution::Available {
                manager: PmId::Winget,
                package: package.id().to_string(),
            },
        },
        Os::Linux => resolve_linux(app, platform),
        Os::Other => unavailable("This operating system isn't supported"),
    }
}

/// Native managers (apt, dnf, pacman) come in a later phase; for now Linux installs use Flatpak.
fn resolve_linux(app: &App, platform: &PlatformInfo) -> Resolution {
    match &app.packages.flatpak {
        Some(_) if !platform.has(PmId::Flatpak) => unavailable("Flatpak isn't installed"),
        Some(id) => Resolution::Available {
            manager: PmId::Flatpak,
            package: id.clone(),
        },
        None => unavailable("No supported package for this system yet"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;

    fn platform(os: Os, managers: &[PmId]) -> PlatformInfo {
        PlatformInfo {
            os,
            arch: "x86_64",
            version: "0.0.0",
            distro: None,
            managers: managers.to_vec(),
        }
    }

    fn app(packages: &str) -> App {
        let json = format!(
            r#"{{ "version": 1, "apps": [{{ "id": "x", "name": "X", "description": "",
                "category": "dev", "packages": {packages} }}] }}"#
        );
        Catalog::parse(&json).unwrap().apps.remove(0)
    }

    #[test]
    fn windows_uses_winget() {
        let resolution = resolve(
            &app(r#"{ "winget": "Git.Git" }"#),
            &platform(Os::Windows, &[PmId::Winget]),
        );
        assert_eq!(
            resolution,
            Resolution::Available {
                manager: PmId::Winget,
                package: "Git.Git".into()
            }
        );
    }

    #[test]
    fn windows_without_winget_is_unavailable() {
        let resolution = resolve(
            &app(r#"{ "winget": "Git.Git" }"#),
            &platform(Os::Windows, &[]),
        );
        assert!(matches!(resolution, Resolution::Unavailable { .. }));
    }

    #[test]
    fn windows_without_mapping_is_unavailable() {
        let resolution = resolve(
            &app(r#"{ "flatpak": "a.b.C" }"#),
            &platform(Os::Windows, &[PmId::Winget]),
        );
        assert!(matches!(resolution, Resolution::Unavailable { .. }));
    }

    #[test]
    fn linux_uses_flatpak() {
        let resolution = resolve(
            &app(r#"{ "winget": "Spotify.Spotify", "flatpak": "com.spotify.Client" }"#),
            &platform(Os::Linux, &[PmId::Flatpak]),
        );
        assert_eq!(
            resolution,
            Resolution::Available {
                manager: PmId::Flatpak,
                package: "com.spotify.Client".into()
            }
        );
    }

    #[test]
    fn linux_apt_only_is_not_supported_yet() {
        let resolution = resolve(
            &app(r#"{ "apt": "git" }"#),
            &platform(Os::Linux, &[PmId::Flatpak]),
        );
        assert!(matches!(resolution, Resolution::Unavailable { .. }));
    }
}

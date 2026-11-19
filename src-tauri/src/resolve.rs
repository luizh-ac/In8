//! Decides which package manager installs each app on the current system.

use serde::Serialize;

use crate::catalog::{App, Target};
use crate::platform::{Os, PlatformInfo};
use crate::pm::PmId;
use crate::wsl::DistroDetails;

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
        Os::Windows => resolve_windows(app, platform),
        Os::Linux => resolve_linux(app, platform),
        Os::Other => unavailable("This operating system isn't supported"),
    }
}

/// winget first; GitHub releases for apps that winget doesn't have.
fn resolve_windows(app: &App, platform: &PlatformInfo) -> Resolution {
    if let Some(package) = &app.packages.winget
        && platform.has(PmId::Winget)
    {
        return Resolution::Available {
            manager: PmId::Winget,
            package: package.id().to_string(),
        };
    }
    if let Some(package) = &app.packages.github
        && platform.has(PmId::Github)
    {
        return Resolution::Available {
            manager: PmId::Github,
            package: package.repo.clone(),
        };
    }
    if app.packages.winget.is_some() {
        unavailable("winget wasn't found")
    } else {
        unavailable("Not available for Windows")
    }
}

/// How the app installs inside the WSL distro. `None` when the app doesn't offer a WSL target.
pub fn resolve_wsl(app: &App, distro: Option<&DistroDetails>) -> Option<Resolution> {
    if !app.targets.contains(&Target::Wsl) {
        return None;
    }
    Some(match (distro, &app.packages.apt) {
        (None, _) => unavailable("Set up a WSL distro first"),
        (Some(distro), _) if !distro.has_user => {
            unavailable("Create your Linux user in the WSL distro first")
        }
        (Some(distro), _) if !distro.apt => {
            unavailable("Only apt-based distros (Ubuntu, Debian) are supported in WSL for now")
        }
        (Some(_), None) => unavailable("No apt package for this app"),
        (Some(_), Some(package)) => Resolution::Available {
            manager: PmId::Apt,
            package: package.clone(),
        },
    })
}

/// Native managers (apt, dnf, pacman) come in a later phase; for now Linux installs use Flatpak.
fn resolve_linux(app: &App, platform: &PlatformInfo) -> Resolution {
    match &app.packages.flatpak {
        Some(_) if !platform.has(PmId::Flatpak) => unavailable("Flatpak isn't installed"),
        Some(package) => Resolution::Available {
            manager: PmId::Flatpak,
            package: package.id().to_string(),
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
    fn windows_falls_back_to_github_releases() {
        let github = r#"{ "github": { "repo": "owner/app", "assetSuffix": ".msi",
            "installer": "msi", "detectName": "App" } }"#;
        let resolution = resolve(
            &app(github),
            &platform(Os::Windows, &[PmId::Winget, PmId::Github]),
        );
        assert_eq!(
            resolution,
            Resolution::Available {
                manager: PmId::Github,
                package: "owner/app".into()
            }
        );
    }

    fn distro(has_user: bool, apt: bool) -> DistroDetails {
        DistroDetails {
            name: "Ubuntu".into(),
            os_name: "Ubuntu 26.04 LTS".into(),
            user: if has_user { "me".into() } else { "root".into() },
            has_user,
            systemd: true,
            apt,
            docker: false,
        }
    }

    #[test]
    fn wsl_target_uses_apt_in_a_ready_distro() {
        let git = app(r#"{ "winget": "Git.Git", "apt": "git" }"#);
        let mut git_wsl = git.clone();
        git_wsl.targets = vec![Target::Host, Target::Wsl];

        assert_eq!(resolve_wsl(&git, Some(&distro(true, true))), None);
        assert_eq!(
            resolve_wsl(&git_wsl, Some(&distro(true, true))),
            Some(Resolution::Available {
                manager: PmId::Apt,
                package: "git".into()
            })
        );
        for not_ready in [None, Some(distro(false, true)), Some(distro(true, false))] {
            assert!(matches!(
                resolve_wsl(&git_wsl, not_ready.as_ref()),
                Some(Resolution::Unavailable { .. })
            ));
        }
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

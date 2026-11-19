//! The app catalog: JSON data embedded in the binary and validated at startup.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

pub const CATEGORIES: &[&str] = &[
    "browsers",
    "dev",
    "communication",
    "media",
    "utilities",
    "gaming",
    "productivity",
    "security",
];

const EMBEDDED_APPS: &str = include_str!("../../catalog/apps.json");

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Catalog {
    pub version: u32,
    pub apps: Vec<App>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct App {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub packages: Packages,
    /// Where the app can be installed when In8 runs on Windows.
    #[serde(default = "default_targets")]
    pub targets: Vec<Target>,
    #[serde(default)]
    pub prefer_linux: PreferLinux,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Packages {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub winget: Option<WingetPackage>,
    /// Direct download from GitHub releases, for Windows apps that aren't in winget.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github: Option<GithubPackage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flatpak: Option<FlatpakPackage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apt: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GithubPackage {
    /// `owner/name` of the repository whose latest release holds the installer.
    pub repo: String,
    /// End of the installer's file name; `{arch}` becomes `x64` or `arm64`.
    pub asset_suffix: String,
    pub installer: InstallerKind,
    /// Name shown in Windows "Installed apps", used to detect the app.
    pub detect_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum InstallerKind {
    Msi,
}

/// A Flatpak app from Flathub (plain ID) or from another remote.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum FlatpakPackage {
    Id(String),
    WithRemote { id: String, remote: FlatpakRemote },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FlatpakRemote {
    pub name: String,
    /// `.flatpakrepo` file describing the remote.
    pub url: String,
}

impl FlatpakPackage {
    pub fn id(&self) -> &str {
        match self {
            Self::Id(id) | Self::WithRemote { id, .. } => id,
        }
    }

    pub fn remote(&self) -> Option<&FlatpakRemote> {
        match self {
            Self::Id(_) => None,
            Self::WithRemote { remote, .. } => Some(remote),
        }
    }
}

/// A winget package: a plain ID (winget source) or an ID with an explicit source.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum WingetPackage {
    Id(String),
    WithSource { id: String, source: WingetSource },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WingetSource {
    #[default]
    Winget,
    Msstore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Target {
    Host,
    Wsl,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PreferLinux {
    #[default]
    Flatpak,
    Native,
}

fn default_targets() -> Vec<Target> {
    vec![Target::Host]
}

impl WingetPackage {
    pub fn id(&self) -> &str {
        match self {
            Self::Id(id) | Self::WithSource { id, .. } => id,
        }
    }

    pub fn source(&self) -> WingetSource {
        match self {
            Self::Id(_) => WingetSource::Winget,
            Self::WithSource { source, .. } => *source,
        }
    }
}

impl Packages {
    fn ids(&self) -> impl Iterator<Item = &str> {
        self.winget
            .as_ref()
            .map(WingetPackage::id)
            .into_iter()
            .chain(self.github.as_ref().map(|github| github.repo.as_str()))
            .chain(self.flatpak.as_ref().map(FlatpakPackage::id))
            .chain(self.apt.as_deref())
    }
}

fn is_slug(value: &str, extra: &[char]) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || extra.contains(&c))
}

fn check_github(github: &GithubPackage) -> std::result::Result<(), &'static str> {
    let valid_repo = github
        .repo
        .split_once('/')
        .is_some_and(|(owner, name)| is_slug(owner, &['_']) && is_slug(name, &['_', '.']));
    if !valid_repo {
        return Err("github.repo must be \"owner/name\"");
    }
    if github.asset_suffix.trim().is_empty() || github.detect_name.trim().is_empty() {
        return Err("github needs assetSuffix and detectName");
    }
    Ok(())
}

fn check_flatpak_remote(remote: &FlatpakRemote) -> std::result::Result<(), &'static str> {
    if !is_slug(&remote.name, &['_']) {
        return Err("flatpak remote name must use letters, digits, dashes and underscores");
    }
    if !remote.url.starts_with("https://") {
        return Err("flatpak remote url must use https");
    }
    Ok(())
}

impl Catalog {
    pub fn load_embedded() -> Result<Self> {
        Self::parse(EMBEDDED_APPS)
    }

    pub fn parse(json: &str) -> Result<Self> {
        let catalog: Catalog =
            serde_json::from_str(json).map_err(|e| Error::Catalog(e.to_string()))?;
        catalog.validate()?;
        Ok(catalog)
    }

    pub fn get(&self, id: &str) -> Option<&App> {
        self.apps.iter().find(|app| app.id == id)
    }

    fn validate(&self) -> Result<()> {
        let mut seen = HashSet::new();
        for app in &self.apps {
            let invalid = |reason: &str| Error::Catalog(format!("app \"{}\": {reason}", app.id));
            let valid_id = !app.id.is_empty()
                && app
                    .id
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
            if !valid_id {
                return Err(invalid("id must use lowercase letters, digits and dashes"));
            }
            if !seen.insert(app.id.as_str()) {
                return Err(invalid("duplicate id"));
            }
            if app.name.trim().is_empty() {
                return Err(invalid("missing name"));
            }
            if !CATEGORIES.contains(&app.category.as_str()) {
                return Err(invalid("unknown category"));
            }
            if app.packages.ids().next().is_none() {
                return Err(invalid("no packages"));
            }
            if app.packages.ids().any(|id| id.trim().is_empty()) {
                return Err(invalid("empty package id"));
            }
            if app.targets.is_empty() {
                return Err(invalid("targets can't be empty"));
            }
            if let Some(github) = &app.packages.github {
                check_github(github).map_err(invalid)?;
            }
            if let Some(remote) = app
                .packages
                .flatpak
                .as_ref()
                .and_then(FlatpakPackage::remote)
            {
                check_flatpak_remote(remote).map_err(invalid)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog_with(app: &str) -> String {
        format!(r#"{{ "version": 1, "apps": [{app}] }}"#)
    }

    const VALID: &str = r#"{ "id": "chrome", "name": "Chrome", "description": "Browser",
        "category": "browsers", "packages": { "winget": "Google.Chrome" } }"#;

    #[test]
    fn embedded_catalog_is_valid() {
        let catalog = Catalog::load_embedded().expect("embedded catalog should be valid");
        assert!(!catalog.apps.is_empty());
    }

    #[test]
    fn defaults_are_applied() {
        let catalog = Catalog::parse(&catalog_with(VALID)).unwrap();
        let app = &catalog.apps[0];
        assert_eq!(app.targets, [Target::Host]);
        assert_eq!(app.prefer_linux, PreferLinux::Flatpak);
        assert_eq!(
            app.packages.winget.as_ref().unwrap().source(),
            WingetSource::Winget
        );
    }

    #[test]
    fn msstore_source_is_parsed() {
        let json = catalog_with(
            r#"{ "id": "netflix", "name": "Netflix", "description": "", "category": "media",
                "packages": { "winget": { "id": "9WZDNCRFJ3TJ", "source": "msstore" } } }"#,
        );
        let catalog = Catalog::parse(&json).unwrap();
        let winget = catalog.apps[0].packages.winget.as_ref().unwrap();
        assert_eq!(winget.id(), "9WZDNCRFJ3TJ");
        assert_eq!(winget.source(), WingetSource::Msstore);
    }

    #[test]
    fn github_and_flatpak_remote_are_parsed() {
        let json = catalog_with(
            r#"{ "id": "launcher", "name": "Launcher", "description": "", "category": "productivity",
                "packages": {
                  "github": { "repo": "owner/craft-launcher", "assetSuffix": "-windows-{arch}.msi",
                              "installer": "msi", "detectName": "Launcher" },
                  "flatpak": { "id": "ai.example.launcher",
                               "remote": { "name": "example-oci", "url": "https://example.com/x.flatpakrepo" } } } }"#,
        );
        let catalog = Catalog::parse(&json).unwrap();
        let packages = &catalog.apps[0].packages;
        assert_eq!(
            packages.github.as_ref().unwrap().repo,
            "owner/craft-launcher"
        );
        let flatpak = packages.flatpak.as_ref().unwrap();
        assert_eq!(flatpak.id(), "ai.example.launcher");
        assert_eq!(flatpak.remote().unwrap().name, "example-oci");
    }

    #[test]
    fn rejects_bad_github_repo_and_insecure_remote() {
        let github = |repo: &str| {
            catalog_with(&format!(
                r#"{{ "id": "x", "name": "X", "description": "", "category": "dev",
                    "packages": {{ "github": {{ "repo": "{repo}", "assetSuffix": ".msi",
                    "installer": "msi", "detectName": "X" }} }} }}"#
            ))
        };
        assert!(Catalog::parse(&github("owner/name")).is_ok());
        assert!(Catalog::parse(&github("owner")).is_err());
        assert!(Catalog::parse(&github("owner/../x")).is_err());

        let remote = catalog_with(
            r#"{ "id": "x", "name": "X", "description": "", "category": "dev",
                "packages": { "flatpak": { "id": "a.b.C",
                "remote": { "name": "r", "url": "http://example.com/x.flatpakrepo" } } } }"#,
        );
        assert!(Catalog::parse(&remote).is_err());
    }

    #[test]
    fn rejects_duplicate_ids() {
        let json = format!(r#"{{ "version": 1, "apps": [{VALID}, {VALID}] }}"#);
        assert!(Catalog::parse(&json).is_err());
    }

    #[test]
    fn rejects_unknown_category() {
        let json = catalog_with(&VALID.replace("browsers", "toys"));
        assert!(Catalog::parse(&json).is_err());
    }

    #[test]
    fn rejects_app_without_packages() {
        let json = catalog_with(
            r#"{ "id": "x", "name": "X", "description": "", "category": "dev", "packages": {} }"#,
        );
        assert!(Catalog::parse(&json).is_err());
    }

    #[test]
    fn rejects_unknown_fields() {
        let json = catalog_with(&VALID.replace("\"name\"", "\"nmae\": \"typo\", \"name\""));
        assert!(Catalog::parse(&json).is_err());
    }

    #[test]
    fn rejects_bad_ids() {
        let json = catalog_with(&VALID.replace("\"chrome\"", "\"Chrome App\""));
        assert!(Catalog::parse(&json).is_err());
    }
}

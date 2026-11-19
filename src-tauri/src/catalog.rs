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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flatpak: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apt: Option<String>,
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
            .chain(self.flatpak.as_deref())
            .chain(self.apt.as_deref())
    }
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

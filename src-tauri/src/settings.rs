//! User settings, saved as `<config>/in8/settings.json`.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::wsl;

/// Mirrored in `src/lib/types.ts` (`Settings`). Unknown or missing fields fall back to defaults,
/// so files from other versions still load.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Windows only: relaunch through UAC on start when not elevated.
    pub always_admin: bool,
    /// The run screen starts with "Dry run" checked.
    pub dry_run_by_default: bool,
    pub theme: Theme,
    /// The WSL distro In8 works with; `None` uses the default distro.
    pub wsl_distro: Option<String>,
    /// Linux: install from the distro's packages before Flatpak when an app has both.
    pub linux_prefer_native: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            always_admin: false,
            dry_run_by_default: true,
            theme: Theme::System,
            wsl_distro: None,
            linux_prefer_native: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn new(config_dir: &Path) -> Self {
        Self {
            path: config_dir.join("settings.json"),
        }
    }

    /// Defaults when the file is missing or unreadable (a broken settings file must never stop In8).
    pub fn load(&self) -> Settings {
        let Ok(content) = fs::read_to_string(&self.path) else {
            return Settings::default();
        };
        match serde_json::from_str::<Settings>(&content) {
            Ok(settings) => sanitize(settings),
            Err(error) => {
                log::warn!("ignoring {}: {error}", self.path.display());
                Settings::default()
            }
        }
    }

    pub fn save(&self, settings: &Settings) -> Result<Settings> {
        let settings = sanitize(settings.clone());
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let temp = self.path.with_extension("json.tmp");
        fs::write(&temp, serde_json::to_string_pretty(&settings)?)?;
        fs::rename(&temp, &self.path)?;
        Ok(settings)
    }
}

fn sanitize(mut settings: Settings) -> Settings {
    if settings
        .wsl_distro
        .as_deref()
        .is_some_and(|name| !wsl::is_distro_name(name))
    {
        settings.wsl_distro = None;
    }
    settings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(name: &str) -> SettingsStore {
        let dir = std::env::temp_dir().join(format!("in8-settings-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        SettingsStore::new(&dir)
    }

    #[test]
    fn missing_or_broken_file_gives_defaults() {
        let store = store("broken");
        assert_eq!(store.load(), Settings::default());
        fs::create_dir_all(store.path.parent().unwrap()).unwrap();
        fs::write(&store.path, "not json").unwrap();
        assert_eq!(store.load(), Settings::default());
        fs::write(&store.path, r#"{ "alwaysAdmin": true, "future": 1 }"#).unwrap();
        let loaded = store.load();
        assert!(loaded.always_admin);
        assert!(loaded.dry_run_by_default);
    }

    #[test]
    fn save_round_trips_and_drops_bad_distro_names() {
        let store = store("save");
        let settings = Settings {
            always_admin: true,
            dry_run_by_default: false,
            theme: Theme::Dark,
            wsl_distro: Some("Ubuntu-24.04".into()),
            linux_prefer_native: true,
        };
        assert_eq!(store.save(&settings).unwrap(), settings);
        assert_eq!(store.load(), settings);

        let bad = Settings {
            wsl_distro: Some("x; rm -rf".into()),
            ..settings
        };
        assert_eq!(store.save(&bad).unwrap().wsl_distro, None);
    }
}

//! Profiles are named selections. Built-in ones ship in `catalog/profiles.json`; the user's own live in
//! `<config>/in8/profiles.json`. The backup file carries the current selection plus the user's profiles,
//! so a freshly formatted PC can get both back.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::catalog::{Catalog, Target};
use crate::debloat::DebloatCatalog;
use crate::error::{Error, Result};
use crate::postinstall::{self, PostInstallCatalog, PostInstallRequest};
use crate::runner::{AppRequest, RunRequest, TweakRequest};
use crate::tweaks::TweakCatalog;
use crate::wsl::{self, WslAction};

const EMBEDDED_PROFILES: &str = include_str!("../../catalog/profiles.json");
const FILE_VERSION: u32 = 1;
const BACKUP_KIND: &str = "in8-backup";
/// Prefix of the IDs of profiles the user saved, so they never clash with built-in ones.
const USER_PREFIX: &str = "user-";
const MAX_NAME_LEN: usize = 60;
/// Backups and profile files are small; refuse anything bigger than this.
const MAX_FILE_SIZE: u64 = 1024 * 1024;

/// Mirrored in `src/lib/types.ts` (`Profile`). Unknown fields are ignored so backups from newer builds open.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub apps: Vec<AppRequest>,
    #[serde(default)]
    pub wsl: Vec<WslAction>,
    /// Tweak IDs to apply.
    #[serde(default)]
    pub tweaks: Vec<String>,
    /// Debloat item IDs to remove.
    #[serde(default)]
    pub debloat: Vec<String>,
    #[serde(default)]
    pub postinstall: Vec<PostInstallRequest>,
}

/// A profile as the frontend sees it. Mirrored in `src/lib/types.ts` (`ProfileEntry`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileEntry {
    #[serde(flatten)]
    pub profile: Profile,
    pub builtin: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProfileFile {
    version: u32,
    profiles: Vec<Profile>,
}

/// The exported file. Unknown fields are ignored so newer files still open in older builds.
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Backup {
    kind: String,
    version: u32,
    #[serde(default)]
    in8_version: String,
    #[serde(default)]
    selection: RunRequest,
    #[serde(default)]
    profiles: Vec<Profile>,
}

/// What an import brought in. Mirrored in `src/lib/types.ts` (`ImportResult`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub selection: RunRequest,
    pub profiles: usize,
    /// App IDs from the file that this build's catalog doesn't know.
    pub unknown_apps: Vec<String>,
}

fn invalid(profile: &str, reason: impl std::fmt::Display) -> Error {
    Error::Profile(format!("profile \"{profile}\": {reason}"))
}

/// Built-in profiles, checked against the catalogs. Any mistake is a bug in the shipped data.
pub fn builtin(
    catalog: &Catalog,
    tweaks: &TweakCatalog,
    debloat: &DebloatCatalog,
    post: &PostInstallCatalog,
) -> Result<Vec<Profile>> {
    let file: ProfileFile =
        serde_json::from_str(EMBEDDED_PROFILES).map_err(|e| Error::Profile(e.to_string()))?;
    let mut ids = HashSet::new();
    for profile in &file.profiles {
        let valid_id = !profile.id.is_empty()
            && !profile.id.starts_with(USER_PREFIX)
            && profile
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !valid_id {
            return Err(invalid(&profile.id, "bad id"));
        }
        if !ids.insert(profile.id.as_str()) {
            return Err(invalid(&profile.id, "duplicate id"));
        }
        if profile.name.trim().is_empty() {
            return Err(invalid(&profile.id, "missing name"));
        }
        let (clean, unknown) = sanitize_selection(catalog, &selection_of(profile));
        if !unknown.is_empty() {
            return Err(invalid(&profile.id, format!("unknown apps {unknown:?}")));
        }
        if clean != selection_of(profile) {
            return Err(invalid(&profile.id, "a target or WSL action isn't valid"));
        }
        if let Some(id) = profile.tweaks.iter().find(|id| tweaks.get(id).is_none()) {
            return Err(invalid(&profile.id, format!("unknown tweak {id}")));
        }
        if let Some(id) = profile.debloat.iter().find(|id| debloat.get(id).is_none()) {
            return Err(invalid(&profile.id, format!("unknown debloat item {id}")));
        }
        postinstall::check_requests(post, catalog, &profile.postinstall)
            .map_err(|reason| invalid(&profile.id, reason))?;
    }
    Ok(file.profiles)
}

fn selection_of(profile: &Profile) -> RunRequest {
    RunRequest {
        apps: profile.apps.clone(),
        wsl: profile.wsl.clone(),
        tweaks: profile
            .tweaks
            .iter()
            .map(|id| TweakRequest {
                id: id.clone(),
                revert: false,
            })
            .collect(),
        debloat: profile.debloat.clone(),
        setup_flatpak: false,
        postinstall: profile.postinstall.clone(),
    }
}

impl Profile {
    fn from_selection(
        id: String,
        name: String,
        description: String,
        selection: RunRequest,
    ) -> Self {
        Self {
            id,
            name,
            description,
            apps: selection.apps,
            wsl: selection.wsl,
            tweaks: selection.tweaks.into_iter().map(|t| t.id).collect(),
            debloat: selection.debloat,
            postinstall: selection.postinstall,
        }
    }
}

/// Drops apps the catalog doesn't know (returned), duplicates and invalid WSL actions;
/// apps that can't go to WSL fall back to the host.
pub fn sanitize_selection(catalog: &Catalog, selection: &RunRequest) -> (RunRequest, Vec<String>) {
    let mut seen = HashSet::new();
    let mut unknown = Vec::new();
    let mut apps = Vec::new();
    for wanted in &selection.apps {
        let Some(app) = catalog.get(&wanted.id) else {
            if !unknown.contains(&wanted.id) {
                unknown.push(wanted.id.clone());
            }
            continue;
        };
        if !seen.insert(app.id.as_str()) {
            continue;
        }
        let target = if app.targets.contains(&wanted.target) {
            wanted.target
        } else {
            Target::Host
        };
        apps.push(AppRequest {
            id: app.id.clone(),
            target,
        });
    }
    let mut actions: Vec<WslAction> = Vec::new();
    for action in &selection.wsl {
        let valid = match action {
            WslAction::InstallDistro { distro } => wsl::is_distro_name(distro),
            _ => true,
        };
        if valid && !actions.contains(action) {
            actions.push(action.clone());
        }
    }
    // Reverts only make sense on the machine where the tweak was applied, so they never travel.
    let mut tweaks: Vec<TweakRequest> = Vec::new();
    for wanted in selection.tweaks.iter().filter(|t| !t.revert) {
        if is_item_id(&wanted.id) && !tweaks.iter().any(|t| t.id == wanted.id) {
            tweaks.push(TweakRequest {
                id: wanted.id.clone(),
                revert: false,
            });
        }
    }
    let mut debloat: Vec<String> = Vec::new();
    for id in &selection.debloat {
        if is_item_id(id) && !debloat.contains(id) {
            debloat.push(id.clone());
        }
    }
    (
        RunRequest {
            apps,
            wsl: actions,
            tweaks,
            debloat,
            // Installing Flatpak is about this machine, so it never travels in profiles or backups.
            setup_flatpak: false,
            postinstall: postinstall::sanitize_requests(&selection.postinstall),
        },
        unknown,
    )
}

/// Tweak and debloat IDs are checked against their catalogs when they're used; here only their shape.
fn is_item_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// `user-` plus a slug of the name, so saving under the same name replaces that profile.
fn user_id(name: &str) -> String {
    let mut slug = String::new();
    for c in name.trim().chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        format!("{USER_PREFIX}profile")
    } else {
        format!("{USER_PREFIX}{slug}")
    }
}

fn clean_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::Profile("give the profile a name".into()));
    }
    Ok(name.chars().take(MAX_NAME_LEN).collect())
}

/// Makes a user profile from untrusted data (the profiles file or a backup).
fn sanitize_user_profile(catalog: &Catalog, profile: &Profile) -> Option<Profile> {
    let name = clean_name(&profile.name).ok()?;
    let (selection, _) = sanitize_selection(catalog, &selection_of(profile));
    Some(Profile::from_selection(
        user_id(&name),
        name,
        profile.description.chars().take(200).collect(),
        selection,
    ))
}

fn read_small_file(path: &Path) -> Result<String> {
    if fs::metadata(path)?.len() > MAX_FILE_SIZE {
        return Err(Error::Backup(
            "the file is too big to be an In8 file".into(),
        ));
    }
    Ok(fs::read_to_string(path)?)
}

/// Writes next to the target and renames, so a crash never leaves half a file.
fn write_atomic(path: &Path, content: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, content)?;
    fs::rename(&temp, path)?;
    Ok(())
}

/// The user's saved profiles.
pub struct UserProfiles {
    path: PathBuf,
}

impl UserProfiles {
    pub fn new(config_dir: &Path) -> Self {
        Self {
            path: config_dir.join("profiles.json"),
        }
    }

    pub fn load(&self, catalog: &Catalog) -> Result<Vec<Profile>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let file: ProfileFile = serde_json::from_str(&read_small_file(&self.path)?)
            .map_err(|e| Error::Profile(format!("{}: {e}", self.path.display())))?;
        let mut profiles: Vec<Profile> = Vec::new();
        for profile in file.profiles {
            if let Some(profile) = sanitize_user_profile(catalog, &profile) {
                profiles.retain(|p| p.id != profile.id);
                profiles.push(profile);
            }
        }
        Ok(profiles)
    }

    fn store(&self, profiles: &[Profile]) -> Result<()> {
        let file = ProfileFile {
            version: FILE_VERSION,
            profiles: profiles.to_vec(),
        };
        write_atomic(&self.path, &serde_json::to_string_pretty(&file)?)
    }

    /// Saves the selection under `name`, replacing a profile with the same name.
    pub fn save(&self, catalog: &Catalog, name: &str, selection: &RunRequest) -> Result<()> {
        let name = clean_name(name)?;
        let (selection, _) = sanitize_selection(catalog, selection);
        if selection.apps.is_empty()
            && selection.wsl.is_empty()
            && selection.tweaks.is_empty()
            && selection.debloat.is_empty()
            && selection.postinstall.is_empty()
        {
            return Err(Error::Profile("select something to save first".into()));
        }
        let mut profiles = self.load(catalog)?;
        let profile = Profile::from_selection(user_id(&name), name, String::new(), selection);
        match profiles.iter_mut().find(|p| p.id == profile.id) {
            Some(existing) => *existing = profile,
            None => profiles.push(profile),
        }
        self.store(&profiles)
    }

    pub fn delete(&self, catalog: &Catalog, id: &str) -> Result<()> {
        let mut profiles = self.load(catalog)?;
        profiles.retain(|p| p.id != id);
        self.store(&profiles)
    }

    /// Adds profiles from a backup; ones with the same name are replaced. Returns how many came in.
    fn merge(&self, catalog: &Catalog, incoming: &[Profile]) -> Result<usize> {
        let mut profiles = self.load(catalog)?;
        let mut count = 0;
        for profile in incoming
            .iter()
            .filter_map(|p| sanitize_user_profile(catalog, p))
        {
            profiles.retain(|p| p.id != profile.id);
            profiles.push(profile);
            count += 1;
        }
        if count > 0 {
            self.store(&profiles)?;
        }
        Ok(count)
    }

    pub fn export(&self, catalog: &Catalog, selection: &RunRequest, path: &Path) -> Result<()> {
        let (selection, _) = sanitize_selection(catalog, selection);
        let backup = Backup {
            kind: BACKUP_KIND.into(),
            version: FILE_VERSION,
            in8_version: env!("CARGO_PKG_VERSION").into(),
            selection,
            profiles: self.load(catalog)?,
        };
        fs::write(path, serde_json::to_string_pretty(&backup)?)?;
        Ok(())
    }

    /// Reads a backup, saves its profiles and returns its selection (for the frontend to apply).
    pub fn import(&self, catalog: &Catalog, path: &Path) -> Result<ImportResult> {
        let backup: Backup = serde_json::from_str(&read_small_file(path)?)
            .map_err(|_| Error::Backup("this isn't an In8 backup file".into()))?;
        if backup.kind != BACKUP_KIND {
            return Err(Error::Backup("this isn't an In8 backup file".into()));
        }
        if backup.version > FILE_VERSION {
            return Err(Error::Backup(
                "this backup comes from a newer In8; update In8 first".into(),
            ));
        }
        let (selection, mut unknown_apps) = sanitize_selection(catalog, &backup.selection);
        for profile in &backup.profiles {
            for app in &profile.apps {
                if catalog.get(&app.id).is_none() && !unknown_apps.contains(&app.id) {
                    unknown_apps.push(app.id.clone());
                }
            }
        }
        let profiles = self.merge(catalog, &backup.profiles)?;
        Ok(ImportResult {
            selection,
            profiles,
            unknown_apps,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> Catalog {
        Catalog::load_embedded().unwrap()
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("in8-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn request(apps: &[(&str, Target)], wsl: Vec<WslAction>) -> RunRequest {
        RunRequest {
            apps: apps
                .iter()
                .map(|(id, target)| AppRequest {
                    id: id.to_string(),
                    target: *target,
                })
                .collect(),
            wsl,
            ..Default::default()
        }
    }

    #[test]
    fn builtin_profiles_are_valid() {
        let profiles = builtin(
            &catalog(),
            &TweakCatalog::load_embedded().unwrap(),
            &DebloatCatalog::load_embedded().unwrap(),
            &PostInstallCatalog::load_embedded().unwrap(),
        )
        .expect("built-in profiles should be valid");
        assert!(profiles.iter().any(|p| p.id == "dev-on-wsl"));
        assert!(profiles.iter().any(|p| p.id == "my-setup"));
    }

    #[test]
    fn my_setup_has_every_catalog_app() {
        let catalog = catalog();
        let profiles = builtin(
            &catalog,
            &TweakCatalog::load_embedded().unwrap(),
            &DebloatCatalog::load_embedded().unwrap(),
            &PostInstallCatalog::load_embedded().unwrap(),
        )
        .unwrap();
        let mine = profiles.iter().find(|p| p.id == "my-setup").unwrap();
        for app in &catalog.apps {
            assert!(
                mine.apps.iter().any(|a| a.id == app.id),
                "My setup is missing {}",
                app.id
            );
        }
    }

    #[test]
    fn sanitize_drops_unknown_apps_and_bad_targets() {
        let selection = request(
            &[
                ("git", Target::Wsl),
                ("chrome", Target::Wsl),
                ("nope", Target::Host),
                ("git", Target::Host),
            ],
            vec![
                WslAction::DockerEngine,
                WslAction::DockerEngine,
                WslAction::InstallDistro {
                    distro: "bad name; rm".into(),
                },
            ],
        );
        let (clean, unknown) = sanitize_selection(&catalog(), &selection);
        assert_eq!(unknown, ["nope"]);
        assert_eq!(
            clean,
            request(
                &[("git", Target::Wsl), ("chrome", Target::Host)],
                vec![WslAction::DockerEngine]
            )
        );
    }

    #[test]
    fn user_ids_are_slugs() {
        assert_eq!(user_id("  My PC (2026)! "), "user-my-pc-2026");
        assert_eq!(user_id("Ação"), "user-a-o");
        assert_eq!(user_id("???"), "user-profile");
    }

    #[test]
    fn save_replaces_same_name_and_delete_removes() {
        let catalog = catalog();
        let store = UserProfiles::new(&temp_dir("save"));
        assert!(store.load(&catalog).unwrap().is_empty());

        store
            .save(
                &catalog,
                "Work",
                &request(&[("chrome", Target::Host)], vec![]),
            )
            .unwrap();
        store
            .save(
                &catalog,
                "work ",
                &request(&[("brave", Target::Host)], vec![]),
            )
            .unwrap();
        let profiles = store.load(&catalog).unwrap();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].apps[0].id, "brave");

        assert!(
            store
                .save(&catalog, "Empty", &RunRequest::default())
                .is_err()
        );
        assert!(
            store
                .save(
                    &catalog,
                    "  ",
                    &request(&[("chrome", Target::Host)], vec![])
                )
                .is_err()
        );

        store.delete(&catalog, "user-work").unwrap();
        assert!(store.load(&catalog).unwrap().is_empty());
    }

    #[test]
    fn backup_round_trip() {
        let catalog = catalog();
        let source = UserProfiles::new(&temp_dir("export"));
        source
            .save(
                &catalog,
                "Dev box",
                &request(&[("nodejs", Target::Wsl)], vec![WslAction::EnableSystemd]),
            )
            .unwrap();
        let file = temp_dir("export-file").join("in8-backup.json");
        let selection = request(&[("chrome", Target::Host), ("git", Target::Wsl)], vec![]);
        source.export(&catalog, &selection, &file).unwrap();

        let target = UserProfiles::new(&temp_dir("import"));
        let result = target.import(&catalog, &file).unwrap();
        assert_eq!(result.selection, selection);
        assert_eq!(result.profiles, 1);
        assert!(result.unknown_apps.is_empty());
        assert_eq!(
            target.load(&catalog).unwrap(),
            source.load(&catalog).unwrap()
        );
    }

    #[test]
    fn import_reports_unknown_apps_and_rejects_other_files() {
        let catalog = catalog();
        let dir = temp_dir("import-bad");
        let store = UserProfiles::new(&dir);

        let file = dir.join("old.json");
        fs::write(
            &file,
            r#"{ "kind": "in8-backup", "version": 1, "future": true,
                 "selection": { "apps": [ { "id": "chrome" }, { "id": "retired-app" } ] },
                 "profiles": [ { "id": "x", "name": "Old", "apps": [ { "id": "gone" } ] } ] }"#,
        )
        .unwrap();
        let result = store.import(&catalog, &file).unwrap();
        assert_eq!(result.selection.apps.len(), 1);
        assert_eq!(result.unknown_apps, ["retired-app", "gone"]);
        assert_eq!(store.load(&catalog).unwrap()[0].id, "user-old");

        let other = dir.join("other.json");
        fs::write(&other, r#"{ "name": "package.json" }"#).unwrap();
        assert!(store.import(&catalog, &other).is_err());

        let newer = dir.join("newer.json");
        fs::write(&newer, r#"{ "kind": "in8-backup", "version": 99 }"#).unwrap();
        assert!(store.import(&catalog, &newer).is_err());
    }
}

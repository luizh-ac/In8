//! Tweaks: small system settings (registry on Windows, gsettings on GNOME) that can be undone.
//! Before a tweak is applied, the values it touches are saved in `<config>/in8/state.json`;
//! reverting restores them exactly (or deletes values and keys that didn't exist).

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::exec::{self, Cmd};
use crate::platform::{Os, PlatformInfo};

const EMBEDDED_TWEAKS: &str = include_str!("../../catalog/tweaks.json");

pub const CATEGORIES: &[&str] = &[
    "appearance",
    "explorer",
    "taskbar",
    "privacy",
    "performance",
];

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TweakCatalog {
    pub version: u32,
    pub tweaks: Vec<Tweak>,
}

/// Mirrored in `src/lib/types.ts` (`Tweak`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Tweak {
    pub id: String,
    pub os: TweakOs,
    /// Linux desktop the tweak is for (only `gnome` for now).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desktop: Option<String>,
    pub category: String,
    pub name: String,
    pub description: String,
    /// Shows up fully only after signing out (or restarting Explorer).
    #[serde(default)]
    pub sign_out: bool,
    pub actions: Vec<TweakAction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TweakOs {
    Windows,
    Linux,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum TweakAction {
    /// Sets one registry value, creating the key if needed. `value: ""` is the key's default value.
    Registry {
        hive: Hive,
        key: String,
        #[serde(default)]
        value: String,
        data: RegData,
    },
    /// Makes a power plan (by GUID) the active one.
    PowerPlan { scheme: String },
    /// `gsettings set <schema> <key> <value>`; `value` is GVariant text, e.g. `'prefer-dark'` or `true`.
    Gsettings {
        schema: String,
        key: String,
        value: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum Hive {
    #[serde(rename = "HKCU")]
    Hkcu,
    #[serde(rename = "HKLM")]
    Hklm,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RegData {
    Dword(u32),
    String(String),
}

impl Hive {
    fn label(self) -> &'static str {
        match self {
            Self::Hkcu => "HKCU",
            Self::Hklm => "HKLM",
        }
    }
}

impl Tweak {
    /// HKLM needs administrator rights.
    pub fn requires_admin(&self) -> bool {
        self.actions.iter().any(|action| {
            matches!(
                action,
                TweakAction::Registry {
                    hive: Hive::Hklm,
                    ..
                }
            )
        })
    }

    /// What applying (or reverting) does, for the run screen and dry runs.
    pub fn describe(&self, revert: bool) -> String {
        let parts: Vec<String> = self
            .actions
            .iter()
            .map(|action| match action {
                TweakAction::Registry {
                    hive,
                    key,
                    value,
                    data,
                } => {
                    let name = if value.is_empty() { "(Default)" } else { value };
                    if revert {
                        format!("restore {}\\{key}\\{name}", hive.label())
                    } else {
                        let data = match data {
                            RegData::Dword(n) => format!("{n} (dword)"),
                            RegData::String(s) => format!("\"{s}\" (string)"),
                        };
                        format!("{}\\{key}\\{name} = {data}", hive.label())
                    }
                }
                TweakAction::PowerPlan { scheme } => {
                    if revert {
                        "powercfg /setactive <previous plan>".to_string()
                    } else {
                        format!("powercfg /setactive {scheme}")
                    }
                }
                TweakAction::Gsettings { schema, key, value } => {
                    if revert {
                        format!("gsettings set {schema} {key} <previous value>")
                    } else {
                        format!("gsettings set {schema} {key} {value}")
                    }
                }
            })
            .collect();
        parts.join("; ")
    }
}

fn is_guid(value: &str) -> bool {
    let groups: Vec<&str> = value.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(group, len)| group.len() == len && group.chars().all(|c| c.is_ascii_hexdigit()))
}

fn is_gsettings_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

impl TweakCatalog {
    pub fn load_embedded() -> Result<Self> {
        Self::parse(EMBEDDED_TWEAKS)
    }

    pub fn parse(json: &str) -> Result<Self> {
        let catalog: TweakCatalog =
            serde_json::from_str(json).map_err(|e| Error::Catalog(format!("tweaks: {e}")))?;
        catalog.validate()?;
        Ok(catalog)
    }

    pub fn get(&self, id: &str) -> Option<&Tweak> {
        self.tweaks.iter().find(|tweak| tweak.id == id)
    }

    fn validate(&self) -> Result<()> {
        let mut seen = HashSet::new();
        for tweak in &self.tweaks {
            let invalid =
                |reason: &str| Error::Catalog(format!("tweak \"{}\": {reason}", tweak.id));
            let valid_id = !tweak.id.is_empty()
                && tweak
                    .id
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
            if !valid_id {
                return Err(invalid("id must use lowercase letters, digits and dashes"));
            }
            if !seen.insert(tweak.id.as_str()) {
                return Err(invalid("duplicate id"));
            }
            if tweak.name.trim().is_empty() || !CATEGORIES.contains(&tweak.category.as_str()) {
                return Err(invalid("missing name or unknown category"));
            }
            if tweak.actions.is_empty() {
                return Err(invalid("no actions"));
            }
            match tweak.os {
                TweakOs::Windows if tweak.desktop.is_some() => {
                    return Err(invalid("desktop is only for Linux tweaks"));
                }
                TweakOs::Linux if tweak.desktop.as_deref() != Some("gnome") => {
                    return Err(invalid("Linux tweaks need desktop \"gnome\""));
                }
                _ => {}
            }
            for action in &tweak.actions {
                match (tweak.os, action) {
                    (TweakOs::Windows, TweakAction::Registry { key, .. }) => {
                        if key.is_empty()
                            || key.starts_with('\\')
                            || key.ends_with('\\')
                            || key.contains("\\\\")
                            || key.contains('/')
                        {
                            return Err(invalid("bad registry key"));
                        }
                    }
                    (TweakOs::Windows, TweakAction::PowerPlan { scheme }) => {
                        if !is_guid(scheme) {
                            return Err(invalid("power plan must be a GUID"));
                        }
                    }
                    (TweakOs::Linux, TweakAction::Gsettings { schema, key, value }) => {
                        if !is_gsettings_name(schema)
                            || !is_gsettings_name(key)
                            || value.trim().is_empty()
                        {
                            return Err(invalid("bad gsettings schema, key or value"));
                        }
                    }
                    _ => return Err(invalid("action doesn't match the tweak's os")),
                }
            }
        }
        Ok(())
    }
}

/// A queued tweak for the runner.
#[derive(Debug, Clone)]
pub struct TweakJob {
    pub tweak: Tweak,
    pub revert: bool,
    pub state: std::sync::Arc<TweakState>,
}

/// A tweak with its state on this machine. Mirrored in `src/lib/types.ts` (`TweakInfo`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TweakInfo {
    #[serde(flatten)]
    pub tweak: Tweak,
    pub requires_admin: bool,
    /// Every value already matches the tweak.
    pub applied: bool,
    /// In8 saved the previous values, so "Revert" can restore them.
    pub revertible: bool,
}

/// Whether tweaks for this OS (and desktop) can run here.
pub fn supported(tweak: &Tweak, platform: &PlatformInfo) -> bool {
    match tweak.os {
        TweakOs::Windows => platform.os == Os::Windows,
        TweakOs::Linux => platform.os == Os::Linux && is_gnome(),
    }
}

fn is_gnome() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP")
        .is_ok_and(|desktop| desktop.to_ascii_lowercase().contains("gnome"))
}

/// Tweaks that can run here, with their current state. Reads the registry / gsettings.
pub fn list(catalog: &TweakCatalog, platform: &PlatformInfo, state: &TweakState) -> Vec<TweakInfo> {
    let saved = state.load();
    catalog
        .tweaks
        .iter()
        .filter(|tweak| supported(tweak, platform))
        .map(|tweak| TweakInfo {
            requires_admin: tweak.requires_admin(),
            applied: tweak.actions.iter().all(is_applied),
            revertible: saved.tweaks.contains_key(&tweak.id),
            tweak: tweak.clone(),
        })
        .collect()
}

fn is_applied(action: &TweakAction) -> bool {
    match action {
        TweakAction::Registry {
            hive,
            key,
            value,
            data,
        } => registry::matches(*hive, key, value, data),
        TweakAction::PowerPlan { scheme } => {
            active_power_plan().is_some_and(|active| active.eq_ignore_ascii_case(scheme))
        }
        TweakAction::Gsettings { schema, key, value } => {
            gsettings_get(schema, key).is_some_and(|current| current == *value)
        }
    }
}

/// The value an action replaced, saved so revert can put it back.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Previous {
    Registry {
        /// `None`: the value didn't exist.
        value: Option<RawValue>,
        /// Keys the tweak created (outermost first); deleted on revert if still empty.
        created_keys: Vec<String>,
    },
    PowerPlan {
        scheme: String,
    },
    Gsettings {
        value: String,
    },
}

/// A registry value exactly as it was (type and bytes).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct RawValue {
    pub vtype: u32,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct StateFile {
    version: u32,
    #[serde(default)]
    tweaks: BTreeMap<String, Vec<Previous>>,
}

/// `<config>/in8/state.json`: previous values of the tweaks In8 applied.
#[derive(Debug)]
pub struct TweakState {
    path: PathBuf,
    lock: Mutex<()>,
}

impl TweakState {
    pub fn new(config_dir: &Path) -> Self {
        Self {
            path: config_dir.join("state.json"),
            lock: Mutex::new(()),
        }
    }

    fn load(&self) -> StateFile {
        fs::read_to_string(&self.path)
            .ok()
            .and_then(|content| serde_json::from_str(&content).ok())
            .unwrap_or_default()
    }

    fn update(&self, change: impl FnOnce(&mut StateFile)) -> std::result::Result<(), String> {
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        let mut file = self.load();
        file.version = 1;
        change(&mut file);
        let write = || -> std::io::Result<()> {
            if let Some(dir) = self.path.parent() {
                fs::create_dir_all(dir)?;
            }
            let temp = self.path.with_extension("json.tmp");
            fs::write(&temp, serde_json::to_string_pretty(&file)?)?;
            fs::rename(&temp, &self.path)
        };
        write().map_err(|e| format!("Couldn't save {}: {e}", self.path.display()))
    }

    fn get(&self, id: &str) -> Option<Vec<Previous>> {
        self.load().tweaks.get(id).cloned()
    }
}

/// Applies a tweak. The previous values are saved first (only the first time, so applying twice
/// still reverts to the original). Lines describing each change go to `log`.
pub fn apply(
    tweak: &Tweak,
    state: &TweakState,
    log: &mut dyn FnMut(String),
) -> std::result::Result<(), String> {
    if state.get(&tweak.id).is_none() {
        let previous = tweak
            .actions
            .iter()
            .map(read_previous)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        state.update(|file| {
            file.tweaks.insert(tweak.id.clone(), previous);
        })?;
    }
    for action in &tweak.actions {
        write(action)?;
        log(describe_action(action));
    }
    Ok(())
}

/// Restores what `apply` saved and forgets it.
pub fn revert(
    tweak: &Tweak,
    state: &TweakState,
    log: &mut dyn FnMut(String),
) -> std::result::Result<(), String> {
    let previous = state.get(&tweak.id).ok_or_else(|| {
        "In8 has no saved values for this tweak, so there's nothing to restore.".to_string()
    })?;
    if previous.len() != tweak.actions.len() {
        return Err("The saved values don't match this tweak anymore.".into());
    }
    for (action, previous) in tweak.actions.iter().zip(&previous).rev() {
        restore(action, previous)?;
        log(format!("Restored {}", describe_target(action)));
    }
    state.update(|file| {
        file.tweaks.remove(&tweak.id);
    })
}

fn describe_target(action: &TweakAction) -> String {
    match action {
        TweakAction::Registry {
            hive, key, value, ..
        } => {
            let name = if value.is_empty() { "(Default)" } else { value };
            format!("{}\\{key}\\{name}", hive.label())
        }
        TweakAction::PowerPlan { .. } => "the power plan".into(),
        TweakAction::Gsettings { schema, key, .. } => format!("{schema} {key}"),
    }
}

fn describe_action(action: &TweakAction) -> String {
    match action {
        TweakAction::Registry { data, .. } => {
            let data = match data {
                RegData::Dword(n) => n.to_string(),
                RegData::String(s) => format!("\"{s}\""),
            };
            format!("Set {} = {data}", describe_target(action))
        }
        TweakAction::PowerPlan { scheme } => format!("Active power plan: {scheme}"),
        TweakAction::Gsettings { value, .. } => {
            format!("Set {} = {value}", describe_target(action))
        }
    }
}

fn read_previous(action: &TweakAction) -> std::result::Result<Previous, String> {
    match action {
        TweakAction::Registry {
            hive, key, value, ..
        } => registry::read(*hive, key, value),
        TweakAction::PowerPlan { .. } => active_power_plan()
            .map(|scheme| Previous::PowerPlan { scheme })
            .ok_or_else(|| "Couldn't read the active power plan.".to_string()),
        TweakAction::Gsettings { schema, key, .. } => gsettings_get(schema, key)
            .map(|value| Previous::Gsettings { value })
            .ok_or_else(|| format!("Couldn't read {schema} {key}.")),
    }
}

fn write(action: &TweakAction) -> std::result::Result<(), String> {
    match action {
        TweakAction::Registry {
            hive,
            key,
            value,
            data,
        } => registry::write(*hive, key, value, data),
        TweakAction::PowerPlan { scheme } => {
            run_quiet(&Cmd::new("powercfg").args(["/setactive", scheme]))
        }
        TweakAction::Gsettings { schema, key, value } => {
            run_quiet(&Cmd::new("gsettings").args(["set", schema, key, value]))
        }
    }
}

fn restore(action: &TweakAction, previous: &Previous) -> std::result::Result<(), String> {
    match (action, previous) {
        (
            TweakAction::Registry {
                hive, key, value, ..
            },
            Previous::Registry {
                value: old,
                created_keys,
            },
        ) => registry::restore(*hive, key, value, old.as_ref(), created_keys),
        (TweakAction::PowerPlan { .. }, Previous::PowerPlan { scheme }) => {
            run_quiet(&Cmd::new("powercfg").args(["/setactive", scheme]))
        }
        (TweakAction::Gsettings { schema, key, .. }, Previous::Gsettings { value }) => {
            run_quiet(&Cmd::new("gsettings").args(["set", schema, key, value]))
        }
        _ => Err("The saved values don't match this tweak anymore.".into()),
    }
}

/// Runs a short command; its output becomes the error message when it fails.
fn run_quiet(cmd: &Cmd) -> std::result::Result<(), String> {
    let output = cmd
        .to_command()
        .output()
        .map_err(|e| format!("Couldn't start {}: {e}", cmd.program))?;
    if output.status.success() {
        return Ok(());
    }
    let text = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let text = if text.is_empty() {
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    } else {
        text
    };
    Err(format!("{} failed: {text}", cmd.display()))
}

/// The GUID in `powercfg /getactivescheme` (the surrounding text is translated, the GUID isn't).
fn active_power_plan() -> Option<String> {
    let output = exec::capture(&Cmd::new("powercfg").arg("/getactivescheme"))?;
    find_guid(&output)
}

fn find_guid(text: &str) -> Option<String> {
    text.split(|c: char| !(c.is_ascii_hexdigit() || c == '-'))
        .find(|word| is_guid(word))
        .map(str::to_lowercase)
}

fn gsettings_get(schema: &str, key: &str) -> Option<String> {
    exec::capture(&Cmd::new("gsettings").args(["get", schema, key])).map(|s| s.trim().to_string())
}

#[cfg(windows)]
mod registry {
    use std::io::ErrorKind;

    use winreg::RegKey;
    use winreg::RegValue;
    use winreg::enums::*;

    use super::{Hive, Previous, RawValue, RegData};

    fn root(hive: Hive) -> RegKey {
        RegKey::predef(match hive {
            Hive::Hkcu => HKEY_CURRENT_USER,
            Hive::Hklm => HKEY_LOCAL_MACHINE,
        })
    }

    fn explain(error: std::io::Error, hive: Hive, key: &str) -> String {
        if error.kind() == ErrorKind::PermissionDenied {
            format!(
                "Access denied to {}\\{key}. Restart In8 as administrator.",
                hive.label()
            )
        } else {
            format!("{}\\{key}: {error}", hive.label())
        }
    }

    fn reg_type(vtype: u32) -> Option<RegType> {
        [
            REG_NONE,
            REG_SZ,
            REG_EXPAND_SZ,
            REG_BINARY,
            REG_DWORD,
            REG_DWORD_BIG_ENDIAN,
            REG_LINK,
            REG_MULTI_SZ,
            REG_RESOURCE_LIST,
            REG_FULL_RESOURCE_DESCRIPTOR,
            REG_RESOURCE_REQUIREMENTS_LIST,
            REG_QWORD,
        ]
        .into_iter()
        .find(|t| t.clone() as u32 == vtype)
    }

    pub fn matches(hive: Hive, key: &str, value: &str, data: &RegData) -> bool {
        let Ok(opened) = root(hive).open_subkey_with_flags(key, KEY_READ) else {
            return false;
        };
        match data {
            RegData::Dword(n) => opened.get_value::<u32, _>(value).is_ok_and(|v| v == *n),
            RegData::String(s) => opened.get_value::<String, _>(value).is_ok_and(|v| v == *s),
        }
    }

    pub fn read(hive: Hive, key: &str, value: &str) -> Result<Previous, String> {
        // Find which parts of the path don't exist yet: those are the keys the tweak will create.
        let parts: Vec<&str> = key.split('\\').collect();
        let mut created_keys = Vec::new();
        for depth in 1..=parts.len() {
            let path = parts[..depth].join("\\");
            if !created_keys.is_empty() {
                created_keys.push(path);
                continue;
            }
            match root(hive).open_subkey_with_flags(&path, KEY_READ) {
                Ok(_) => {}
                Err(e) if e.kind() == ErrorKind::NotFound => created_keys.push(path),
                Err(e) => return Err(explain(e, hive, key)),
            }
        }
        let previous = if created_keys.is_empty() {
            let opened = root(hive)
                .open_subkey_with_flags(key, KEY_READ)
                .map_err(|e| explain(e, hive, key))?;
            match opened.get_raw_value(value) {
                Ok(raw) => Some(RawValue {
                    vtype: raw.vtype as u32,
                    bytes: raw.bytes.into_owned(),
                }),
                Err(e) if e.kind() == ErrorKind::NotFound => None,
                Err(e) => return Err(explain(e, hive, key)),
            }
        } else {
            None
        };
        Ok(Previous::Registry {
            value: previous,
            created_keys,
        })
    }

    pub fn write(hive: Hive, key: &str, value: &str, data: &RegData) -> Result<(), String> {
        let (opened, _) = root(hive)
            .create_subkey(key)
            .map_err(|e| explain(e, hive, key))?;
        let result = match data {
            RegData::Dword(n) => opened.set_value(value, n),
            RegData::String(s) => opened.set_value(value, s),
        };
        result.map_err(|e| explain(e, hive, key))
    }

    pub fn restore(
        hive: Hive,
        key: &str,
        value: &str,
        old: Option<&RawValue>,
        created_keys: &[String],
    ) -> Result<(), String> {
        match root(hive).open_subkey_with_flags(key, KEY_READ | KEY_SET_VALUE) {
            Ok(opened) => {
                let result = match old {
                    Some(raw) => {
                        let vtype = reg_type(raw.vtype)
                            .ok_or_else(|| format!("unknown registry type {}", raw.vtype))?;
                        opened.set_raw_value(
                            value,
                            &RegValue {
                                bytes: raw.bytes.clone().into(),
                                vtype,
                            },
                        )
                    }
                    None => match opened.delete_value(value) {
                        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
                        other => other,
                    },
                };
                result.map_err(|e| explain(e, hive, key))?;
            }
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => return Err(explain(e, hive, key)),
        }
        // Remove the keys the tweak created, innermost first, but only while they're empty.
        for path in created_keys.iter().rev() {
            let Ok(opened) = root(hive).open_subkey_with_flags(path, KEY_READ) else {
                continue;
            };
            let empty = opened
                .query_info()
                .is_ok_and(|info| info.sub_keys == 0 && info.values == 0);
            drop(opened);
            if !empty {
                break;
            }
            root(hive)
                .delete_subkey(path)
                .map_err(|e| explain(e, hive, path))?;
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod registry {
    use super::{Hive, Previous, RawValue, RegData};

    const UNSUPPORTED: &str = "The registry only exists on Windows.";

    pub fn matches(_: Hive, _: &str, _: &str, _: &RegData) -> bool {
        false
    }

    pub fn read(_: Hive, _: &str, _: &str) -> Result<Previous, String> {
        Err(UNSUPPORTED.into())
    }

    pub fn write(_: Hive, _: &str, _: &str, _: &RegData) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }

    pub fn restore(
        _: Hive,
        _: &str,
        _: &str,
        _: Option<&RawValue>,
        _: &[String],
    ) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_tweaks_are_valid() {
        let catalog = TweakCatalog::load_embedded().expect("embedded tweaks should be valid");
        assert!(catalog.get("dark-mode").is_some());
        assert!(catalog.get("hide-widgets").unwrap().requires_admin());
        assert!(!catalog.get("dark-mode").unwrap().requires_admin());
    }

    #[test]
    fn rejects_mismatched_actions_and_bad_values() {
        let tweak = |os: &str, action: &str| {
            format!(
                r#"{{ "version": 1, "tweaks": [ {{ "id": "x", "os": "{os}", {desktop} "category": "privacy",
                    "name": "X", "description": "", "actions": [ {action} ] }} ] }}"#,
                desktop = if os == "linux" {
                    r#""desktop": "gnome","#
                } else {
                    ""
                }
            )
        };
        let reg = r#"{ "type": "registry", "hive": "HKCU", "key": "Software\\X", "value": "V", "data": { "dword": 1 } }"#;
        assert!(TweakCatalog::parse(&tweak("windows", reg)).is_ok());
        assert!(TweakCatalog::parse(&tweak("linux", reg)).is_err());
        let bad_key = reg.replace("Software\\\\X", "\\\\Software");
        assert!(TweakCatalog::parse(&tweak("windows", &bad_key)).is_err());
        let power = r#"{ "type": "powerPlan", "scheme": "not-a-guid" }"#;
        assert!(TweakCatalog::parse(&tweak("windows", power)).is_err());
        let gsettings =
            r#"{ "type": "gsettings", "schema": "org.gnome.x", "key": "k; rm", "value": "true" }"#;
        assert!(TweakCatalog::parse(&tweak("linux", gsettings)).is_err());
    }

    #[test]
    fn finds_guid_in_localized_powercfg_output() {
        let pt = "GUID do Esquema de Energia: 381b4222-f694-41f0-9685-ff5bb260df2e  (Equilibrado)";
        assert_eq!(
            find_guid(pt).as_deref(),
            Some("381b4222-f694-41f0-9685-ff5bb260df2e")
        );
        assert_eq!(find_guid("no plan"), None);
    }

    #[test]
    fn describe_shows_registry_paths() {
        let catalog = TweakCatalog::load_embedded().unwrap();
        let text = catalog.get("show-file-extensions").unwrap().describe(false);
        assert_eq!(
            text,
            "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced\\HideFileExt = 0 (dword)"
        );
        let menu = catalog.get("classic-context-menu").unwrap().describe(false);
        assert!(menu.ends_with("InprocServer32\\(Default) = \"\" (string)"));
    }

    /// Applies and reverts a tweak on a throwaway key under HKCU\Software\In8Test (deleted afterwards).
    #[cfg(windows)]
    #[test]
    fn registry_apply_and_revert_round_trip() {
        let base = format!("Software\\In8Test-{}", std::process::id());
        let tweak = Tweak {
            id: "test".into(),
            os: TweakOs::Windows,
            desktop: None,
            category: "privacy".into(),
            name: "Test".into(),
            description: String::new(),
            sign_out: false,
            actions: vec![
                TweakAction::Registry {
                    hive: Hive::Hkcu,
                    key: format!("{base}\\Deep\\Key"),
                    value: "New".into(),
                    data: RegData::Dword(7),
                },
                TweakAction::Registry {
                    hive: Hive::Hkcu,
                    key: base.clone(),
                    value: "Existing".into(),
                    data: RegData::String("after".into()),
                },
            ],
        };
        let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
        let (existing, _) = hkcu.create_subkey(&base).unwrap();
        existing.set_value("Existing", &"before").unwrap();
        existing.set_value("Other", &1u32).unwrap();

        let dir = std::env::temp_dir().join(format!("in8-tweak-state-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let state = TweakState::new(&dir);
        let mut lines = Vec::new();

        apply(&tweak, &state, &mut |line| lines.push(line)).unwrap();
        assert!(tweak.actions.iter().all(is_applied));
        // Applying again keeps the original values saved.
        apply(&tweak, &state, &mut |_| {}).unwrap();

        revert(&tweak, &state, &mut |line| lines.push(line)).unwrap();
        assert!(hkcu.open_subkey(format!("{base}\\Deep")).is_err());
        let value: String = hkcu
            .open_subkey(&base)
            .unwrap()
            .get_value("Existing")
            .unwrap();
        assert_eq!(value, "before");
        assert!(state.get("test").is_none());
        assert!(revert(&tweak, &state, &mut |_| {}).is_err());

        hkcu.delete_subkey_all(&base).unwrap();
        assert_eq!(lines.len(), 4);
    }
}

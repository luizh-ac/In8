//! Commands exposed to the frontend. Kept thin: the logic lives in the other modules.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::catalog::{App, Catalog};
use crate::debloat::{self, DebloatCatalog, DebloatInfo};
use crate::elevation::{self, Relaunch};
use crate::error::{Error, Result};
use crate::platform::{Os, PlatformInfo};
use crate::pm::{self, PmId, apt};
use crate::postinstall::{self, PostInstallCatalog, PostInstallState, PostTask};
use crate::profiles::{self, ImportResult, Profile, ProfileEntry, UserProfiles};
use crate::resolve::{Resolution, resolve, resolve_wsl};
use crate::runner::{self, RunEvent, RunRequest, RunSummary, SystemContext};
use crate::settings::{Settings, SettingsStore};
use crate::tweaks::{self, TweakCatalog, TweakInfo, TweakState};
use crate::wsl::{self, DistroDetails, OnlineDistro, WslConfig, WslStatus};

pub struct AppState {
    pub catalog: Arc<Catalog>,
    pub platform: Arc<PlatformInfo>,
    pub running: Arc<AtomicBool>,
    /// The WSL distro In8 works with, from the last `get_wsl_status`.
    pub wsl: RwLock<Option<DistroDetails>>,
    pub profiles: Arc<ProfileState>,
    pub tweaks: Arc<TweakCatalog>,
    pub debloat: Arc<DebloatCatalog>,
    pub tweak_state: Arc<TweakState>,
    pub postinstall: Arc<PostInstallCatalog>,
    pub settings: Mutex<SettingsStore>,
    pub admin: AdminStatus,
    /// The selection handed over to the elevated copy by "Restart as administrator".
    pub pending_selection: PathBuf,
}

/// Mirrored in `src/lib/types.ts` (`AdminStatus`).
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminStatus {
    pub elevated: bool,
    /// "Always run as administrator" is on but the UAC prompt was declined at start.
    pub declined: bool,
    /// Only Windows can relaunch elevated.
    pub can_elevate: bool,
}

pub struct ProfileState {
    pub catalog: Arc<Catalog>,
    pub builtin: Vec<Profile>,
    /// Locked for each read-modify-write of the user's profiles file.
    pub user: Mutex<UserProfiles>,
}

impl ProfileState {
    fn user(&self) -> MutexGuard<'_, UserProfiles> {
        self.user
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl AppState {
    fn wsl_distro(&self) -> Option<DistroDetails> {
        self.wsl
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

/// Mirrored in `src/lib/types.ts` (`CatalogApp`).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    #[serde(flatten)]
    pub app: App,
    pub resolution: Resolution,
    /// How the app installs inside WSL, for apps that offer that target on Windows.
    pub wsl_resolution: Option<Resolution>,
}

async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Result<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| Error::Join(e.to_string()))
}

#[tauri::command]
pub fn get_platform(state: State<'_, AppState>) -> PlatformInfo {
    (*state.platform).clone()
}

#[tauri::command]
pub fn get_catalog(state: State<'_, AppState>) -> Vec<CatalogEntry> {
    let distro = state.wsl_distro();
    let windows = state.platform.os == Os::Windows;
    state
        .catalog
        .apps
        .iter()
        .map(|app| CatalogEntry {
            app: app.clone(),
            resolution: resolve(app, &state.platform),
            wsl_resolution: windows.then(|| resolve_wsl(app, distro.as_ref())).flatten(),
        })
        .collect()
}

/// Reads the WSL setup (a second or two: it may start the distro) and remembers the active distro.
/// `None` outside Windows.
#[tauri::command]
pub async fn get_wsl_status(
    state: State<'_, AppState>,
    distro: Option<String>,
) -> Result<Option<WslStatus>> {
    if state.platform.os != Os::Windows {
        return Ok(None);
    }
    let status = blocking(move || wsl::status(distro.as_deref())).await?;
    *state
        .wsl
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = status.active.clone();
    Ok(Some(status))
}

#[tauri::command]
pub async fn get_online_distros() -> Result<Vec<OnlineDistro>> {
    blocking(wsl::online_distros).await
}

/// IDs of catalog apps already installed inside the active WSL distro.
#[tauri::command]
pub async fn get_wsl_installed(state: State<'_, AppState>) -> Result<Vec<String>> {
    let Some(distro) = state.wsl_distro().filter(DistroDetails::is_ready) else {
        return Ok(Vec::new());
    };
    let catalog = Arc::clone(&state.catalog);
    blocking(move || {
        let candidates: Vec<(&App, String)> = catalog
            .apps
            .iter()
            .filter_map(|app| match resolve_wsl(app, Some(&distro)) {
                Some(Resolution::Available { package, .. }) => Some((app, package)),
                _ => None,
            })
            .collect();
        let packages: Vec<&str> = candidates.iter().map(|(_, p)| p.as_str()).collect();
        let installed: HashSet<String> = crate::exec::capture_stdout(&wsl::inside(
            &distro.name,
            false,
            apt::query_cmd(&packages),
        ))
        .map(|output| apt::parse_query(&output).into_iter().collect())
        .unwrap_or_default();
        candidates
            .into_iter()
            .filter(|(_, package)| installed.contains(package))
            .map(|(app, _)| app.id.clone())
            .collect()
    })
    .await
}

#[tauri::command]
pub fn open_distro_terminal(distro: String) -> Result<()> {
    wsl::open_terminal(&distro)
}

#[tauri::command]
pub async fn get_wslconfig() -> Result<WslConfig> {
    blocking(wsl::read_wslconfig).await?
}

#[tauri::command]
pub async fn preview_wslconfig(config: WslConfig) -> Result<String> {
    blocking(move || wsl::preview_wslconfig(&config)).await?
}

#[tauri::command]
pub async fn save_wslconfig(config: WslConfig) -> Result<()> {
    blocking(move || wsl::save_wslconfig(&config)).await?
}

/// IDs of catalog apps already installed. Slow (asks every package manager), so it runs off the main thread.
#[tauri::command]
pub async fn get_installed(state: State<'_, AppState>) -> Result<Vec<String>> {
    let catalog = Arc::clone(&state.catalog);
    let platform = Arc::clone(&state.platform);
    blocking(move || installed_app_ids(&catalog, &platform)).await?
}

#[tauri::command]
pub async fn start_run(
    state: State<'_, AppState>,
    request: RunRequest,
    dry_run: bool,
    on_event: Channel<RunEvent>,
) -> Result<RunSummary> {
    if state.running.swap(true, Ordering::SeqCst) {
        return Err(Error::AlreadyRunning);
    }
    let running = Arc::clone(&state.running);
    let catalog = Arc::clone(&state.catalog);
    let platform = Arc::clone(&state.platform);
    let distro = state.wsl_distro();
    let tweak_catalog = Arc::clone(&state.tweaks);
    let debloat_catalog = Arc::clone(&state.debloat);
    let tweak_state = Arc::clone(&state.tweak_state);
    let post_catalog = Arc::clone(&state.postinstall);
    let result = blocking(move || {
        let context = SystemContext {
            tweaks: &tweak_catalog,
            debloat: &debloat_catalog,
            state: &tweak_state,
        };
        // Debloat first, then apps and WSL, then post-install, then tweaks.
        let (mut tasks, tweak_tasks) = runner::plan_system(&context, &platform, &request)?;
        tasks.extend(runner::plan(
            &catalog,
            &platform,
            distro.as_ref(),
            &request,
        )?);
        let apt_update_planned = tasks.iter().any(|task| task.id == "wsl-apt-update");
        tasks.extend(postinstall::plan(
            &post_catalog,
            &platform,
            distro.as_ref(),
            &request.postinstall,
            apt_update_planned,
        )?);
        tasks.extend(tweak_tasks);
        // Steps asked for twice (e.g. the VS Code WSL extension) run once, where they first appear.
        let mut seen = HashSet::new();
        tasks.retain(|task| seen.insert(task.id.clone()));
        Ok(runner::run(&tasks, dry_run, |event| {
            let _ = on_event.send(event);
        }))
    })
    .await;
    running.store(false, Ordering::SeqCst);
    result?
}

/// Post-install tasks for this system plus what's already set up (Git identity, SSH keys, extensions).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostInstallInfo {
    pub tasks: Vec<PostTask>,
    pub state: PostInstallState,
}

#[tauri::command]
pub async fn get_postinstall(state: State<'_, AppState>) -> Result<PostInstallInfo> {
    let catalog = Arc::clone(&state.postinstall);
    let platform = Arc::clone(&state.platform);
    let distro = state.wsl_distro();
    blocking(move || PostInstallInfo {
        tasks: catalog.tasks.clone(),
        state: postinstall::read_state(&platform, distro.as_ref()),
    })
    .await
}

/// Tweaks for this system with their current state. Reads the registry or gsettings.
#[tauri::command]
pub async fn get_tweaks(state: State<'_, AppState>) -> Result<Vec<TweakInfo>> {
    let catalog = Arc::clone(&state.tweaks);
    let platform = Arc::clone(&state.platform);
    let tweak_state = Arc::clone(&state.tweak_state);
    blocking(move || tweaks::list(&catalog, &platform, &tweak_state)).await
}

/// Debloat items with whether they're installed. Empty outside Windows. Takes a few seconds.
#[tauri::command]
pub async fn get_debloat(state: State<'_, AppState>) -> Result<Vec<DebloatInfo>> {
    let catalog = Arc::clone(&state.debloat);
    let platform = Arc::clone(&state.platform);
    blocking(move || debloat::list(&catalog, &platform)).await?
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state
        .settings
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .load()
}

#[tauri::command]
pub fn save_settings(state: State<'_, AppState>, settings: Settings) -> Result<Settings> {
    state
        .settings
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .save(&settings)
}

#[tauri::command]
pub fn get_admin_status(state: State<'_, AppState>) -> AdminStatus {
    state.admin
}

/// Saves the selection for the elevated copy, asks for UAC and closes this window if it started.
/// Returns `false` when the UAC prompt was declined.
#[tauri::command]
pub async fn restart_as_admin(
    app: AppHandle,
    state: State<'_, AppState>,
    selection: RunRequest,
) -> Result<bool> {
    if state.running.load(Ordering::SeqCst) {
        return Err(Error::AlreadyRunning);
    }
    let path = state.pending_selection.clone();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(&path, serde_json::to_string(&selection)?)?;
    match blocking(|| elevation::relaunch_elevated(&[])).await? {
        Ok(Relaunch::Started) => {
            app.exit(0);
            Ok(true)
        }
        Ok(Relaunch::Declined) => {
            let _ = fs::remove_file(&path);
            Ok(false)
        }
        Err(error) => {
            let _ = fs::remove_file(&path);
            Err(error.into())
        }
    }
}

/// The selection left by "Restart as administrator", once (the file is deleted).
#[tauri::command]
pub fn take_pending_selection(state: State<'_, AppState>) -> Option<RunRequest> {
    let path = &state.pending_selection;
    let content = fs::read_to_string(path).ok()?;
    let _ = fs::remove_file(path);
    let selection: RunRequest = serde_json::from_str(&content).ok()?;
    Some(profiles::sanitize_selection(&state.catalog, &selection).0)
}

fn profile_entries(state: &ProfileState) -> Result<Vec<ProfileEntry>> {
    let user = state.user().load(&state.catalog)?;
    Ok(state
        .builtin
        .iter()
        .map(|profile| ProfileEntry {
            profile: profile.clone(),
            builtin: true,
        })
        .chain(user.into_iter().map(|profile| ProfileEntry {
            profile,
            builtin: false,
        }))
        .collect())
}

/// Built-in profiles first, then the user's.
#[tauri::command]
pub async fn get_profiles(state: State<'_, AppState>) -> Result<Vec<ProfileEntry>> {
    let profiles = Arc::clone(&state.profiles);
    blocking(move || profile_entries(&profiles)).await?
}

/// Saves the selection as a user profile (same name replaces it) and returns all profiles.
#[tauri::command]
pub async fn save_profile(
    state: State<'_, AppState>,
    name: String,
    selection: RunRequest,
) -> Result<Vec<ProfileEntry>> {
    let profiles = Arc::clone(&state.profiles);
    blocking(move || {
        profiles.user().save(&profiles.catalog, &name, &selection)?;
        profile_entries(&profiles)
    })
    .await?
}

#[tauri::command]
pub async fn delete_profile(state: State<'_, AppState>, id: String) -> Result<Vec<ProfileEntry>> {
    let profiles = Arc::clone(&state.profiles);
    blocking(move || {
        profiles.user().delete(&profiles.catalog, &id)?;
        profile_entries(&profiles)
    })
    .await?
}

/// Asks where to save, then writes the selection and the user's profiles. `None` if cancelled.
#[tauri::command]
pub async fn export_backup(
    app: AppHandle,
    state: State<'_, AppState>,
    selection: RunRequest,
) -> Result<Option<String>> {
    let profiles = Arc::clone(&state.profiles);
    blocking(move || {
        let Some(path) = app
            .dialog()
            .file()
            .set_title("Export In8 backup")
            .add_filter("In8 backup", &["json"])
            .set_file_name("in8-backup.json")
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let path = path.into_path().map_err(|e| Error::Dialog(e.to_string()))?;
        profiles
            .user()
            .export(&profiles.catalog, &selection, &path)?;
        Ok(Some(path.display().to_string()))
    })
    .await?
}

/// Asks for a backup file, saves its profiles and returns its selection. `None` if cancelled.
#[tauri::command]
pub async fn import_backup(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<ImportResult>> {
    let profiles = Arc::clone(&state.profiles);
    blocking(move || {
        let Some(path) = app
            .dialog()
            .file()
            .set_title("Import In8 backup")
            .add_filter("In8 backup", &["json"])
            .blocking_pick_file()
        else {
            return Ok(None);
        };
        let path = path.into_path().map_err(|e| Error::Dialog(e.to_string()))?;
        profiles.user().import(&profiles.catalog, &path).map(Some)
    })
    .await?
}

fn installed_app_ids(catalog: &Catalog, platform: &PlatformInfo) -> Result<Vec<String>> {
    // Which package each app would use here, grouped by manager.
    let mut candidates: HashMap<PmId, Vec<(&App, String)>> = HashMap::new();
    for app in &catalog.apps {
        if let Resolution::Available { manager, package } = resolve(app, platform) {
            candidates.entry(manager).or_default().push((app, package));
        }
    }

    let mut installed_ids = Vec::new();
    for (manager, apps) in candidates {
        let app_refs: Vec<&App> = apps.iter().map(|(app, _)| *app).collect();
        let installed: HashSet<String> = pm::installed_packages(manager, &app_refs)?
            .into_iter()
            .map(|id| id.to_lowercase())
            .collect();
        installed_ids.extend(
            apps.iter()
                .filter(|(_, package)| installed.contains(&package.to_lowercase()))
                .map(|(app, _)| app.id.clone()),
        );
    }
    // Keep catalog order for a stable result.
    Ok(catalog
        .apps
        .iter()
        .filter(|app| installed_ids.contains(&app.id))
        .map(|app| app.id.clone())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Talks to the real package managers. Run with `cargo test -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn detects_installed_apps_on_this_machine() {
        let catalog = Catalog::load_embedded().unwrap();
        let platform = PlatformInfo::detect();
        let installed = installed_app_ids(&catalog, &platform).unwrap();
        println!("managers: {:?}", platform.managers);
        println!("installed: {installed:?}");
        assert!(!platform.managers.is_empty());
    }

    /// Plans the "Dev on WSL" setup against the real distro and dry-runs it (nothing executes).
    /// Run with `cargo test real_wsl_plan -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_wsl_plan_dry_run() {
        let catalog = Catalog::load_embedded().unwrap();
        let platform = PlatformInfo::detect();
        let distro = wsl::status(None).active;
        let request = RunRequest {
            apps: ["git", "nodejs", "python", "rust"]
                .iter()
                .map(|id| runner::AppRequest {
                    id: id.to_string(),
                    target: crate::catalog::Target::Wsl,
                })
                .chain(std::iter::once(runner::AppRequest {
                    id: "vscode".into(),
                    target: crate::catalog::Target::Host,
                }))
                .collect(),
            wsl: vec![
                wsl::WslAction::DockerEngine,
                wsl::WslAction::VscodeExtension,
            ],
            ..Default::default()
        };
        let tasks = runner::plan(&catalog, &platform, distro.as_ref(), &request).unwrap();
        runner::run(&tasks, true, |event| {
            if let RunEvent::TaskStarted { label, command, .. } = event {
                println!("PLAN {label}: {command}");
            }
        });
    }

    /// Reads the real WSL distro: status, apt detection and stdin passthrough. Read-only.
    /// Run with `cargo test real_wsl -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_wsl_distro_round_trip() {
        let status = wsl::status(None);
        let distro = status.active.expect("a WSL distro");
        println!("{distro:?}");
        assert!(distro.is_ready());

        let catalog = Catalog::load_embedded().unwrap();
        let packages: Vec<&str> = catalog
            .apps
            .iter()
            .filter_map(|app| match resolve_wsl(app, Some(&distro)) {
                Some(Resolution::Available { .. }) => app.packages.apt.as_deref(),
                _ => None,
            })
            .collect();
        let output = crate::exec::capture_stdout(&wsl::inside(
            &distro.name,
            false,
            apt::query_cmd(&packages),
        ))
        .unwrap();
        println!(
            "wsl candidates: {packages:?}, installed: {:?}",
            apt::parse_query(&output)
        );

        let echoed = crate::exec::Cmd::new("cat").input("hello from In8\n");
        let mut events = Vec::new();
        let task = runner::Task {
            id: "echo".into(),
            label: "echo".into(),
            action: pm::InstallAction::Command(wsl::inside(&distro.name, false, echoed)),
            kind: runner::TaskKind::Setup,
        };
        runner::run(&[task], false, |event| events.push(event));
        assert!(events.iter().any(|event| matches!(
            event,
            RunEvent::Output { line, .. } if line == "hello from In8"
        )));
    }
}

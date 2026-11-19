//! Commands exposed to the frontend. Kept thin: the logic lives in the other modules.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use serde::Serialize;
use tauri::State;
use tauri::ipc::Channel;

use crate::catalog::{App, Catalog};
use crate::error::{Error, Result};
use crate::platform::{Os, PlatformInfo};
use crate::pm::{self, PmId, apt};
use crate::resolve::{Resolution, resolve, resolve_wsl};
use crate::runner::{self, RunEvent, RunRequest, RunSummary};
use crate::wsl::{self, DistroDetails, OnlineDistro, WslConfig, WslStatus};

pub struct AppState {
    pub catalog: Arc<Catalog>,
    pub platform: Arc<PlatformInfo>,
    pub running: Arc<AtomicBool>,
    /// The WSL distro In8 works with, from the last `get_wsl_status`.
    pub wsl: RwLock<Option<DistroDetails>>,
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
    let result = blocking(move || {
        let tasks = runner::plan(&catalog, &platform, distro.as_ref(), &request)?;
        Ok(runner::run(&tasks, dry_run, |event| {
            let _ = on_event.send(event);
        }))
    })
    .await;
    running.store(false, Ordering::SeqCst);
    result?
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

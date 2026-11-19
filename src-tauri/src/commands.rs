//! Commands exposed to the frontend. Kept thin: the logic lives in the other modules.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use tauri::State;
use tauri::ipc::Channel;

use crate::catalog::{App, Catalog};
use crate::error::{Error, Result};
use crate::platform::PlatformInfo;
use crate::pm::{self, PmId};
use crate::resolve::{Resolution, resolve};
use crate::runner::{self, RunEvent, RunSummary};

pub struct AppState {
    pub catalog: Arc<Catalog>,
    pub platform: Arc<PlatformInfo>,
    pub running: Arc<AtomicBool>,
}

/// Mirrored in `src/lib/types.ts` (`CatalogApp`).
#[derive(Serialize)]
pub struct CatalogEntry {
    #[serde(flatten)]
    pub app: App,
    pub resolution: Resolution,
}

#[tauri::command]
pub fn get_platform(state: State<'_, AppState>) -> PlatformInfo {
    (*state.platform).clone()
}

#[tauri::command]
pub fn get_catalog(state: State<'_, AppState>) -> Vec<CatalogEntry> {
    state
        .catalog
        .apps
        .iter()
        .map(|app| CatalogEntry {
            app: app.clone(),
            resolution: resolve(app, &state.platform),
        })
        .collect()
}

/// IDs of catalog apps already installed. Slow (asks every package manager), so it runs off the main thread.
#[tauri::command]
pub async fn get_installed(state: State<'_, AppState>) -> Result<Vec<String>> {
    let catalog = Arc::clone(&state.catalog);
    let platform = Arc::clone(&state.platform);
    tauri::async_runtime::spawn_blocking(move || installed_app_ids(&catalog, &platform))
        .await
        .map_err(|e| Error::Join(e.to_string()))?
}

#[tauri::command]
pub async fn start_install(
    state: State<'_, AppState>,
    app_ids: Vec<String>,
    dry_run: bool,
    on_event: Channel<RunEvent>,
) -> Result<RunSummary> {
    let tasks = runner::plan_install(&state.catalog, &state.platform, &app_ids)?;
    if state.running.swap(true, Ordering::SeqCst) {
        return Err(Error::AlreadyRunning);
    }
    let running = Arc::clone(&state.running);
    let result = tauri::async_runtime::spawn_blocking(move || {
        runner::run(&tasks, dry_run, |event| {
            let _ = on_event.send(event);
        })
    })
    .await;
    running.store(false, Ordering::SeqCst);
    result.map_err(|e| Error::Join(e.to_string()))
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
}

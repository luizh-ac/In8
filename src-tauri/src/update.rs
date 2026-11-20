//! Self-update from GitHub Releases. Releases are signed in CI with the private key whose public half
//! is in `tauri.conf.json` (`plugins.updater.pubkey`); an update with a bad signature is refused.

use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

use crate::error::{Error, Result};

/// Mirrored in `src/lib/types.ts` (`UpdateInfo`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    /// Release notes.
    pub notes: Option<String>,
}

fn failed(error: impl std::fmt::Display) -> Error {
    Error::Update(error.to_string())
}

/// Asks GitHub for a newer release. `None` when In8 is up to date.
pub async fn check(app: &AppHandle) -> Result<Option<UpdateInfo>> {
    let update = app
        .updater()
        .map_err(failed)?
        .check()
        .await
        .map_err(failed)?;
    Ok(update.map(|update| UpdateInfo {
        version: update.version,
        current_version: update.current_version,
        notes: update.body,
    }))
}

/// Downloads and installs the newer release, then restarts In8 (on Windows the installer closes it).
pub async fn install(app: &AppHandle) -> Result<()> {
    let Some(update) = app
        .updater()
        .map_err(failed)?
        .check()
        .await
        .map_err(failed)?
    else {
        return Ok(());
    };
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(failed)?;
    app.restart();
}

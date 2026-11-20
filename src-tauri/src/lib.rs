mod catalog;
mod commands;
mod error;
mod exec;
mod ini;
mod platform;
mod pm;
mod profiles;
mod resolve;
mod runner;
mod wsl;

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            let catalog = Arc::new(catalog::Catalog::load_embedded()?);
            let builtin_profiles = profiles::builtin(&catalog)?;
            let config_dir = app.path().config_dir()?.join("in8");
            let platform = platform::PlatformInfo::detect();
            log::info!(
                "In8 {} on {:?}, managers: {:?}",
                platform.version,
                platform.os,
                platform.managers
            );
            app.manage(commands::AppState {
                catalog: Arc::clone(&catalog),
                platform: Arc::new(platform),
                running: Arc::new(AtomicBool::new(false)),
                wsl: RwLock::new(None),
                profiles: Arc::new(commands::ProfileState {
                    catalog,
                    builtin: builtin_profiles,
                    user: Mutex::new(profiles::UserProfiles::new(&config_dir)),
                }),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_platform,
            commands::get_catalog,
            commands::get_installed,
            commands::get_wsl_status,
            commands::get_online_distros,
            commands::get_wsl_installed,
            commands::open_distro_terminal,
            commands::get_wslconfig,
            commands::preview_wslconfig,
            commands::save_wslconfig,
            commands::start_run,
            commands::get_profiles,
            commands::save_profile,
            commands::delete_profile,
            commands::export_backup,
            commands::import_backup,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the In8 application");
}

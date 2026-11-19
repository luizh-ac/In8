mod catalog;
mod commands;
mod error;
mod exec;
mod platform;
mod pm;
mod resolve;
mod runner;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            let catalog = catalog::Catalog::load_embedded()?;
            let platform = platform::PlatformInfo::detect();
            log::info!(
                "In8 {} on {:?}, managers: {:?}",
                platform.version,
                platform.os,
                platform.managers
            );
            app.manage(commands::AppState {
                catalog: Arc::new(catalog),
                platform: Arc::new(platform),
                running: Arc::new(AtomicBool::new(false)),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_platform,
            commands::get_catalog,
            commands::get_installed,
            commands::start_install,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the In8 application");
}

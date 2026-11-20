mod catalog;
mod commands;
mod debloat;
mod elevation;
mod error;
mod exec;
mod ini;
mod platform;
mod pm;
mod postinstall;
mod profiles;
mod resolve;
mod root;
mod runner;
mod settings;
mod tweaks;
mod wsl;

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

use tauri::Manager;

/// `in8 --root-helper`: the Linux root helper started through pkexec (see `root.rs`), not the app.
pub fn root_helper_requested() -> bool {
    std::env::args().nth(1).as_deref() == Some(root::HELPER_FLAG)
}

pub fn run_root_helper() -> i32 {
    root::run_helper()
}

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
            let config_dir = app.path().config_dir()?.join("in8");
            let settings = settings::SettingsStore::new(&config_dir);
            let platform = platform::PlatformInfo::detect_with(settings.load().linux_prefer_native);

            let elevated = elevation::is_elevated();
            let mut elevation_declined = false;
            if platform.os == platform::Os::Windows && settings.load().always_admin && !elevated {
                if cfg!(debug_assertions) {
                    // `tauri dev` stops the dev server when this process exits, so the elevated copy
                    // would open a blank window. Test the setting with a release build.
                    log::info!("\"Always run as administrator\" is on, but ignored in dev builds");
                } else {
                    match elevation::relaunch_elevated(&[]) {
                        Ok(elevation::Relaunch::Started) => std::process::exit(0),
                        Ok(elevation::Relaunch::Declined) => elevation_declined = true,
                        Err(error) => log::warn!("couldn't relaunch as administrator: {error}"),
                    }
                }
            }

            let catalog = Arc::new(catalog::Catalog::load_embedded()?);
            let tweak_catalog = Arc::new(tweaks::TweakCatalog::load_embedded()?);
            let debloat_catalog = Arc::new(debloat::DebloatCatalog::load_embedded()?);
            let post_catalog = Arc::new(postinstall::PostInstallCatalog::load_embedded()?);
            let builtin_profiles =
                profiles::builtin(&catalog, &tweak_catalog, &debloat_catalog, &post_catalog)?;
            log::info!(
                "In8 {} on {:?}, managers: {:?}, elevated: {elevated}",
                platform.version,
                platform.os,
                platform.managers
            );
            app.manage(commands::AppState {
                catalog: Arc::clone(&catalog),
                platform: RwLock::new(Arc::new(platform)),
                running: Arc::new(AtomicBool::new(false)),
                wsl: RwLock::new(None),
                profiles: Arc::new(commands::ProfileState {
                    catalog,
                    builtin: builtin_profiles,
                    user: Mutex::new(profiles::UserProfiles::new(&config_dir)),
                }),
                tweaks: tweak_catalog,
                debloat: debloat_catalog,
                tweak_state: Arc::new(tweaks::TweakState::new(&config_dir)),
                postinstall: post_catalog,
                settings: Mutex::new(settings),
                admin: commands::AdminStatus {
                    elevated,
                    declined: elevation_declined,
                    can_elevate: cfg!(windows),
                },
                pending_selection: config_dir.join("pending-selection.json"),
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
            commands::get_tweaks,
            commands::get_debloat,
            commands::get_postinstall,
            commands::get_settings,
            commands::save_settings,
            commands::get_admin_status,
            commands::refresh_platform,
            commands::restart_as_admin,
            commands::take_pending_selection,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the In8 application");
}

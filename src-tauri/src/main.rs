// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if in8_lib::root_helper_requested() {
        std::process::exit(in8_lib::run_root_helper());
    }
    in8_lib::run();
}

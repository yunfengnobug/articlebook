pub mod commands;

use articlebook_engine::init_logging;
use std::path::PathBuf;
use tauri::Manager;

pub struct AppState {
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub log_dir: PathBuf,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let log_dir = app.path().app_log_dir()?;
            let data_dir = app.path().app_data_dir()?;
            let cache_dir = app.path().app_cache_dir()?;
            std::fs::create_dir_all(&log_dir)?;
            std::fs::create_dir_all(&data_dir)?;
            std::fs::create_dir_all(&cache_dir)?;
            init_logging(&log_dir)?;
            app.manage(AppState {
                data_dir,
                cache_dir,
                log_dir,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_paths,
            commands::suggest_output_dir,
            commands::list_providers,
            commands::preview_links,
            commands::run_list_export,
            commands::run_account_export,
            commands::open_output_dir,
            commands::read_text_file,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

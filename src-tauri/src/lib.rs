pub mod commands;

use articlebook_engine::init_logging;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::Manager;

pub struct AppState {
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub log_dir: PathBuf,
    /// Last usable mp.weixin.qq.com history or console URL seen in the login window.
    /// The cookie itself is not kept here.
    pub wechat_page: Mutex<String>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
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
                wechat_page: Mutex::new(String::new()),
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
            commands::describe_wechat_page_url,
            commands::load_wechat_session,
            commands::save_wechat_session,
            commands::clear_wechat_session,
            commands::open_wechat_login_window,
            commands::open_wechat_login_site,
            commands::preview_wechat_history,
            commands::check_for_update,
            commands::install_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

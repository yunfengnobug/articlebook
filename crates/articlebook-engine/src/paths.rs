use crate::error::EngineError;
use directories::{ProjectDirs, UserDirs};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct AppDirs {
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub log_dir: PathBuf,
}

/// Directories used by the CLI. The desktop shell prefers Tauri's path resolver.
pub fn cli_app_dirs() -> Result<AppDirs, EngineError> {
    let project = ProjectDirs::from("app", "ArticleBook", "ArticleBook")
        .ok_or_else(|| EngineError::Message("无法定位本机数据目录".into()))?;
    let data_dir = project.data_dir().to_path_buf();
    Ok(AppDirs {
        cache_dir: project.cache_dir().to_path_buf(),
        log_dir: data_dir.join("logs"),
        data_dir,
    })
}

pub fn today_stamp() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

pub fn suggest_output_dir() -> PathBuf {
    let date = today_stamp();
    if let Some(dirs) = UserDirs::new() {
        if let Some(docs) = dirs.document_dir() {
            return docs.join("ArticleBook").join(date);
        }
    }
    if let Ok(dirs) = cli_app_dirs() {
        return dirs.data_dir.join("exports").join(date);
    }
    PathBuf::from("ArticleBook").join(date)
}

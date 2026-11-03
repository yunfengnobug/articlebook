use std::path::Path;

/// Recoverable failure from the archive engine.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("{0}")]
    Message(String),
    #[error("无效链接：{0}")]
    BadUrl(String),
    #[error("抓取失败：{0}")]
    Fetch(String),
    #[error("提取正文失败：{0}")]
    Extract(String),
    #[error("读写文件失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("数据库失败：{0}")]
    Db(String),
    #[error("导出 EPUB 失败：{0}")]
    Epub(String),
}

impl From<rusqlite::Error> for EngineError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Db(value.to_string())
    }
}

impl EngineError {
    pub fn reading(path: &Path, err: std::io::Error) -> Self {
        Self::Message(format!("无法读取 {}：{err}", path.display()))
    }
}

use std::fs::OpenOptions;
use std::io;
use std::path::Path;
use std::sync::Mutex;

/// Append info logs to `{log_dir}/articlebook.log`. Safe to call more than once.
pub fn init_logging(log_dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(log_dir)?;
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join("articlebook.log"))?;
    let _ = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_max_level(tracing::Level::INFO)
        .with_writer(Mutex::new(file))
        .try_init();
    Ok(())
}

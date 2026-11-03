use crate::AppState;
use articlebook_engine::{
    describe_providers, export_urls, parse_url_list, provider_by_id,
    suggest_output_dir as suggest_dir, AccountRequest, ExportOptions, JobReport, NormalizeOptions,
    ParsedUrls, ProgressEvent, ProgressSink, ProviderInfo, ReqwestClient, USER_AGENT,
};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSettings {
    pub output_dir: String,
    pub book_title: String,
    pub book_author: String,
    pub localize_images: bool,
    pub export_epub: bool,
    pub strip_tracking: bool,
    pub concurrency: u32,
    pub retries: u32,
    pub delay_ms: u64,
    pub use_cache: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListExportRequest {
    pub urls_text: String,
    #[serde(flatten)]
    pub settings: ExportSettings,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountExportRequest {
    pub provider_id: String,
    pub account_name: String,
    pub account_id: String,
    pub links_text: String,
    pub links_path: Option<String>,
    #[serde(flatten)]
    pub settings: ExportSettings,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathsDto {
    pub data_dir: String,
    pub cache_dir: String,
    pub log_dir: String,
}

struct TauriSink(AppHandle);

impl ProgressSink for TauriSink {
    fn emit(&self, event: ProgressEvent) {
        let _ = self.0.emit("job-progress", event);
    }
}

#[tauri::command]
pub fn app_paths(state: State<'_, AppState>) -> PathsDto {
    PathsDto {
        data_dir: state.data_dir.display().to_string(),
        cache_dir: state.cache_dir.display().to_string(),
        log_dir: state.log_dir.display().to_string(),
    }
}

#[tauri::command]
pub fn suggest_output_dir() -> String {
    suggest_dir().display().to_string()
}

#[tauri::command]
pub fn list_providers() -> Vec<ProviderInfo> {
    describe_providers()
}

#[tauri::command]
pub fn preview_links(text: String, strip_tracking: bool) -> ParsedUrls {
    parse_url_list(
        &text,
        &NormalizeOptions {
            strip_tracking,
            ..NormalizeOptions::default()
        },
    )
}

#[tauri::command]
pub async fn run_list_export(
    app: AppHandle,
    state: State<'_, AppState>,
    req: ListExportRequest,
) -> Result<JobReport, String> {
    let opts = build_opts(&req.settings, &state)?;
    run_export(app, opts, req.urls_text, None).await
}

#[tauri::command]
pub async fn run_account_export(
    app: AppHandle,
    state: State<'_, AppState>,
    req: AccountExportRequest,
) -> Result<JobReport, String> {
    let provider = provider_by_id(&req.provider_id).map_err(|err| err.to_string())?;
    let listing = provider
        .list_links(&AccountRequest {
            account_id: req.account_id,
            account_name: req.account_name,
            links_text: req.links_text,
            links_file: req
                .links_path
                .filter(|path| !path.trim().is_empty())
                .map(PathBuf::from),
        })
        .map_err(|err| err.to_string())?;
    let mut settings = req.settings;
    if settings.book_title.trim().is_empty() {
        settings.book_title = listing.account_name.clone();
    }
    if settings.book_author.trim().is_empty() {
        settings.book_author = listing.account_name.clone();
    }
    let opts = build_opts(&settings, &state)?;
    run_export(app, opts, listing.urls_text, Some(listing.source_note)).await
}

async fn run_export(
    app: AppHandle,
    opts: ExportOptions,
    urls_text: String,
    note: Option<String>,
) -> Result<JobReport, String> {
    let client =
        Arc::new(ReqwestClient::new(USER_AGENT, opts.timeout()).map_err(|err| err.to_string())?);
    let sink = Arc::new(TauriSink(app));
    let mut report = export_urls(&urls_text, opts, client, sink)
        .await
        .map_err(|err| err.to_string())?;
    if let Some(note) = note {
        report.warnings.insert(0, note);
    }
    Ok(report)
}

fn build_opts(settings: &ExportSettings, state: &AppState) -> Result<ExportOptions, String> {
    let output = settings.output_dir.trim();
    if output.is_empty() {
        return Err("请选择输出目录".into());
    }
    let mut opts = ExportOptions::new(PathBuf::from(output), state.cache_dir.join("articles.db"));
    opts.book_title = if settings.book_title.trim().is_empty() {
        format!("阅读归档 {}", articlebook_engine::today_stamp())
    } else {
        settings.book_title.trim().to_string()
    };
    opts.book_author = if settings.book_author.trim().is_empty() {
        "个人归档".into()
    } else {
        settings.book_author.trim().to_string()
    };
    opts.localize_images = settings.localize_images;
    opts.export_epub = settings.export_epub;
    opts.strip_tracking = settings.strip_tracking;
    opts.concurrency = settings.concurrency.clamp(1, 6) as usize;
    opts.retries = settings.retries.clamp(0, 5);
    opts.delay_ms = settings.delay_ms.clamp(0, 10_000);
    opts.use_cache = settings.use_cache;
    opts.timeout_secs = 25;
    Ok(opts)
}

#[tauri::command]
pub fn read_text_file(path: String) -> Result<String, String> {
    let path = PathBuf::from(path.trim());
    let bytes =
        std::fs::read(&path).map_err(|err| format!("无法读取 {}：{err}", path.display()))?;
    Ok(articlebook_engine::decode_text_bytes(&bytes))
}

#[tauri::command]
pub fn open_output_dir(path: String) -> Result<(), String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("没有输出目录".into());
    }
    let path = PathBuf::from(path);
    if !path.exists() {
        return Err("目录不存在".into());
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&path)
            .spawn()
            .map_err(|err| err.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&path)
            .spawn()
            .map_err(|err| err.to_string())?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map_err(|err| err.to_string())?;
    }
    Ok(())
}

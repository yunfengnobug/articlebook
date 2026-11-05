use crate::AppState;
use articlebook_engine::{
    complete_session, describe_providers, describe_wechat_page, export_urls, load_session,
    parse_url_list, prepare_session, provider_by_id, save_session, session_file,
    suggest_output_dir as suggest_dir, AccountProvider, AccountRequest, ExportOptions, JobReport,
    NormalizeOptions, PageHint, ParsedUrls, ProgressEvent, ProgressSink, ProviderInfo,
    ReqwestClient, SessionSummary, WechatWebProvider, USER_AGENT,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_updater::UpdaterExt;

const WECHAT_LOGIN_LABEL: &str = "wechat-login";
const WECHAT_LOGIN_SITE: &str = "https://mp.weixin.qq.com/";

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
    #[serde(default)]
    pub history_limit: u32,
    #[serde(flatten)]
    pub settings: ExportSettings,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveWechatSessionRequest {
    pub page_url: String,
    pub cookie: String,
    pub account_name: String,
    #[serde(default)]
    pub clear_cookie: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WechatPageEvent {
    url: String,
    hint: PageHint,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPreview {
    pub account_name: String,
    pub count: usize,
    pub urls: Vec<String>,
    pub source_note: String,
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
    let session_path = wechat_session_path(&req.provider_id, &state);
    let listing = provider
        .list_links(&AccountRequest {
            account_id: req.account_id,
            account_name: req.account_name,
            links_text: req.links_text,
            links_file: req
                .links_path
                .filter(|path| !path.trim().is_empty())
                .map(PathBuf::from),
            session_file: session_path,
            history_limit: req.history_limit,
        })
        .await
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

#[tauri::command]
pub fn describe_wechat_page_url(url: String) -> PageHint {
    describe_wechat_page(&url)
}

#[tauri::command]
pub fn load_wechat_session(state: State<'_, AppState>) -> Result<SessionSummary, String> {
    let path = session_file(&state.data_dir);
    if !path.exists() {
        return Ok(SessionSummary::missing(&path));
    }
    let session = load_session(&path).map_err(|err| err.to_string())?;
    Ok(SessionSummary::from_session(&session, &path))
}

#[tauri::command]
pub fn clear_wechat_session(state: State<'_, AppState>) -> Result<SessionSummary, String> {
    let path = session_file(&state.data_dir);
    if path.exists() {
        std::fs::remove_file(&path).map_err(|err| format!("无法删除会话：{err}"))?;
    }
    Ok(SessionSummary::missing(&path))
}

#[tauri::command]
pub async fn save_wechat_session(
    app: AppHandle,
    state: State<'_, AppState>,
    req: SaveWechatSessionRequest,
) -> Result<SessionSummary, String> {
    let path = session_file(&state.data_dir);
    let captured = state
        .wechat_page
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .clone();
    let previous = load_session(&path).ok();
    let manual_cookie = !req.cookie.trim().is_empty();
    let mut page_url = req.page_url.trim().to_string();
    // In-app save follows the page on screen. A stale "usable" URL left in the
    // form must not turn a short /s/ link or the admin home into a saved login.
    if !manual_cookie {
        if let Some(window) = app.get_webview_window(WECHAT_LOGIN_LABEL) {
            if let Ok(url) = window.url() {
                if url.host_str() == Some("mp.weixin.qq.com") {
                    page_url = url.to_string();
                }
            }
        }
    }
    if page_url.is_empty() {
        page_url = captured;
    }
    if page_url.is_empty() {
        return Err("请先在登录窗口打开历史消息，或在「高级：手动 Cookie」里粘贴地址。".into());
    }
    let mut cookie = req.cookie.trim().to_string();
    if cookie.is_empty() && !req.clear_cookie {
        if let Some(window) = app.get_webview_window(WECHAT_LOGIN_LABEL) {
            if let Ok(header) = read_mp_cookie(window).await {
                if !header.is_empty() {
                    cookie = header;
                }
            }
        }
    }
    let session = prepare_session(
        &page_url,
        &cookie,
        &req.account_name,
        previous.as_ref(),
        req.clear_cookie,
    )
    .map_err(|err| err.to_string())?;
    let session = complete_session(session)
        .await
        .map_err(|err| err.to_string())?;
    save_session(&path, &session).map_err(|err| err.to_string())?;
    Ok(SessionSummary::from_session(&session, &path))
}

#[tauri::command]
pub async fn preview_wechat_history(
    state: State<'_, AppState>,
    account_name: String,
    history_limit: u32,
) -> Result<HistoryPreview, String> {
    let path = session_file(&state.data_dir);
    let provider = WechatWebProvider::live().map_err(|err| err.to_string())?;
    let listing = provider
        .list_links(&AccountRequest {
            account_id: String::new(),
            account_name,
            links_text: String::new(),
            links_file: None,
            session_file: Some(path),
            history_limit,
        })
        .await
        .map_err(|err| err.to_string())?;
    let parsed = parse_url_list(
        &listing.urls_text,
        &NormalizeOptions {
            strip_tracking: true,
            ..NormalizeOptions::default()
        },
    );
    Ok(HistoryPreview {
        account_name: listing.account_name,
        count: parsed.urls.len(),
        urls: parsed.urls,
        source_note: listing.source_note,
    })
}

/// Opens https://mp.weixin.qq.com/ in its own window.
///
/// The command is async on purpose. On Windows, `WebviewWindowBuilder` inside a
/// synchronous command deadlocks WebView2 (wry#583): the controller never
/// finishes, the new window stays white, and the UI thread never handles
/// WM_CLOSE. Async commands run off that thread, so the page can load and the
/// title-bar close button keeps working at any time.
#[tauri::command]
pub async fn open_wechat_login_window(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(WECHAT_LOGIN_LABEL) {
        let _ = window.set_closable(true);
        let _ = window.unminimize();
        window.show().map_err(|err| err.to_string())?;
        window.set_focus().map_err(|err| err.to_string())?;
        return Ok(());
    }
    let app_nav = app.clone();
    let url = WECHAT_LOGIN_SITE
        .parse::<tauri::Url>()
        .map_err(|err| err.to_string())?;
    // Sign-in page only. capabilities/wechat-login.json grants this window no
    // commands and no remote IPC allowlist, so mp.weixin.qq.com cannot call the app.
    WebviewWindowBuilder::new(&app, WECHAT_LOGIN_LABEL, WebviewUrl::External(url))
        .title("微信网页登录")
        .inner_size(980.0, 760.0)
        .min_inner_size(640.0, 480.0)
        .decorations(true)
        .closable(true)
        .focused(true)
        .on_navigation(move |url| {
            // NavigationStarting runs on the WebView2 UI thread. Calling
            // emit() here re-enters that thread: the closure never returns,
            // the load stays blank, and the window cannot be closed.
            schedule_wechat_page(&app_nav, url.clone());
            true
        })
        .build()
        .map_err(|err| err.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn open_wechat_login_site() -> Result<(), String> {
    open_constant_site(WECHAT_LOGIN_SITE)
}

fn wechat_session_path(provider_id: &str, state: &AppState) -> Option<PathBuf> {
    match provider_id {
        "wechat-web" | "wechat-history" => Some(session_file(&state.data_dir)),
        _ => None,
    }
}

fn schedule_wechat_page(app: &AppHandle, url: tauri::Url) {
    if url.host_str() != Some("mp.weixin.qq.com") {
        return;
    }
    let app = app.clone();
    // Detach before touching another webview. spawn_blocking does not run the
    // closure inline, so NavigationStarting can return and allow the load.
    let _ = tauri::async_runtime::spawn_blocking(move || {
        record_wechat_page(&app, &url);
    });
}

fn record_wechat_page(app: &AppHandle, url: &tauri::Url) {
    if url.host_str() != Some("mp.weixin.qq.com") {
        return;
    }
    let text = url.to_string();
    let hint = describe_wechat_page(&text);
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut slot) = state.wechat_page.lock() {
            if hint.usable {
                *slot = text.clone();
            } else {
                slot.clear();
            }
        }
    }
    let _ = app.emit("wechat-page", WechatPageEvent { url: text, hint });
}

async fn read_mp_cookie(window: tauri::WebviewWindow) -> Result<String, String> {
    let cookies = tauri::async_runtime::spawn_blocking(move || {
        let root = "https://mp.weixin.qq.com/"
            .parse::<tauri::Url>()
            .expect("mp host");
        // Root first, then the open page, so a path-scoped session cookie wins.
        let mut urls = vec![root.clone()];
        if let Ok(current) = window.url() {
            if current.host_str() == Some("mp.weixin.qq.com") && current != root {
                urls.push(current);
            }
        }
        let mut merged = Vec::new();
        let mut last_err = None;
        for url in urls {
            match window.cookies_for_url(url) {
                Ok(list) => merged.extend(list),
                Err(err) => last_err = Some(err.to_string()),
            }
        }
        if merged.is_empty() {
            if let Some(err) = last_err {
                return Err(format!("读取登录窗口 Cookie 失败：{err}"));
            }
        }
        Ok(merged)
    })
    .await
    .map_err(|err| format!("读取登录窗口 Cookie 失败：{err}"))??;
    Ok(cookie_header(&cookies))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub available: bool,
    pub version: String,
    pub message: String,
}

#[tauri::command]
pub async fn check_for_update(app: AppHandle) -> Result<UpdateStatus, String> {
    let update = app
        .updater()
        .map_err(|err| err.to_string())?
        .check()
        .await
        .map_err(|err| err.to_string())?;
    match update {
        Some(update) => {
            let version = update.version.clone();
            Ok(UpdateStatus {
                available: true,
                version: version.clone(),
                message: format!(
                    "发现新版本 {version}。下载安装后，Windows 会退出应用并完成安装。"
                ),
            })
        }
        None => Ok(UpdateStatus {
            available: false,
            version: String::new(),
            message: "已是最新版本。".into(),
        }),
    }
}

#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    let Some(update) = app
        .updater()
        .map_err(|err| err.to_string())?
        .check()
        .await
        .map_err(|err| err.to_string())?
    else {
        return Err("没有可安装的更新。".into());
    };
    update
        .download_and_install(|_chunk, _total| {}, || {})
        .await
        .map_err(|err| err.to_string())
}

fn cookie_header(cookies: &[tauri::webview::Cookie<'static>]) -> String {
    let mut names = Vec::new();
    let mut values = Vec::new();
    for cookie in cookies {
        let name = cookie.name();
        let value = cookie.value();
        if name.is_empty()
            || value.is_empty()
            || name.contains(['\n', '\r', ';', '='])
            || value.contains(['\n', '\r'])
        {
            continue;
        }
        if let Some(pos) = names.iter().position(|existing: &String| existing == name) {
            values[pos] = value.to_string();
        } else {
            names.push(name.to_string());
            values.push(value.to_string());
        }
    }
    names
        .iter()
        .zip(values)
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("; ")
}

fn open_constant_site(url: &str) -> Result<(), String> {
    if url != WECHAT_LOGIN_SITE {
        return Err("只会打开 mp.weixin.qq.com 首页。".into());
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .map_err(|err| err.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|err| err.to_string())?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map_err(|err| err.to_string())?;
    }
    Ok(())
}

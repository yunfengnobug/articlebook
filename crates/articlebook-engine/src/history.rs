//! History lists from a WeChat web session the user already signed into.
//!
//! Two official pages are accepted:
//! - the public account "历史消息" page (`/mp/profile_ext`)
//! - the publisher console's article list (`/cgi-bin/appmsg`), for an account the user administers
//!
//! The engine does not log in, does not launch WMPF or Frida, and does not send a WeChat client
//! user agent. Callers pass query credentials and an optional cookie copied from that login.
//!
//! `history_limit == 0` ([`history_limit_or_default`]) means every article that login can read.
//! Paging stops when `can_msg_continue` / `can_continue` is false, when a page has no articles,
//! or at [`SAFETY_MAX_ARTICLES`] / [`SAFETY_MAX_PAGES`]. The desktop provider waits 800ms between pages.

use crate::error::EngineError;
use crate::fetch::USER_AGENT;
use crate::rate::Pace;
use crate::urlutil::{normalize_url, NormalizeOptions};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;
use url::form_urlencoded::Target;
use url::Url;

pub const SESSION_FILE_NAME: &str = "wechat-web-session.json";
const PAGE_SIZE: usize = 10;

/// Most articles one pull will keep.
///
/// `history_limit == 0` ("全部") stops here even if WeChat still reports another page.
/// A positive limit ("仅最近 N 篇") is also clamped to this ceiling.
pub const SAFETY_MAX_ARTICLES: usize = 5_000;

/// Most history responses one pull will send.
///
/// Each response asks for [`PAGE_SIZE`] items (10). This is high enough to reach
/// [`SAFETY_MAX_ARTICLES`] when pages are short, and it ends the loop if
/// `can_continue` never turns false. The desktop provider still waits 800ms
/// between these requests.
pub const SAFETY_MAX_PAGES: usize = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HistoryKind {
    ReaderHistory,
    OwnerConsole,
}

impl HistoryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReaderHistory => "reader-history",
            Self::OwnerConsole => "owner-console",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::ReaderHistory => "历史消息",
            Self::OwnerConsole => "公众平台图文",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WechatWebSession {
    pub version: u32,
    pub kind: HistoryKind,
    pub account_name: String,
    pub biz: String,
    pub uin: String,
    pub key: String,
    pub pass_ticket: String,
    pub token: String,
    pub cookie: String,
    pub source_url: String,
    pub captured_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub saved: bool,
    pub kind: String,
    pub account_name: String,
    pub biz: String,
    pub has_cookie: bool,
    pub has_key: bool,
    pub has_token: bool,
    pub captured_at: String,
    pub source_url: String,
    pub path: String,
}

impl SessionSummary {
    pub fn missing(path: &Path) -> Self {
        Self {
            saved: false,
            kind: String::new(),
            account_name: String::new(),
            biz: String::new(),
            has_cookie: false,
            has_key: false,
            has_token: false,
            captured_at: String::new(),
            source_url: String::new(),
            path: path.display().to_string(),
        }
    }

    pub fn from_session(session: &WechatWebSession, path: &Path) -> Self {
        Self {
            saved: true,
            kind: session.kind.as_str().to_string(),
            account_name: session.account_name.clone(),
            biz: session.biz.clone(),
            has_cookie: !session.cookie.trim().is_empty(),
            has_key: !session.key.trim().is_empty(),
            has_token: !session.token.trim().is_empty(),
            captured_at: session.captured_at.clone(),
            source_url: session.source_url.clone(),
            path: path.display().to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageHint {
    pub usable: bool,
    pub message: String,
    pub kind: String,
    pub biz: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    pub url: String,
    pub title: String,
}

#[async_trait]
pub trait HistoryClient: Send + Sync {
    async fn get_text(
        &self,
        url: &str,
        cookie: &str,
        user_agent: &str,
        referer: &str,
    ) -> Result<String, EngineError>;
}

pub struct ReqwestHistoryClient {
    client: reqwest::Client,
}

impl ReqwestHistoryClient {
    pub fn new(timeout: Duration) -> Result<Self, EngineError> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|err| EngineError::Fetch(err.to_string()))?;
        Ok(Self { client })
    }
}

#[async_trait]
impl HistoryClient for ReqwestHistoryClient {
    async fn get_text(
        &self,
        url: &str,
        cookie: &str,
        user_agent: &str,
        referer: &str,
    ) -> Result<String, EngineError> {
        let parsed = Url::parse(url).map_err(|err| EngineError::BadUrl(err.to_string()))?;
        if !is_mp_host(parsed.host_str()) {
            return Err(EngineError::Message(
                "历史目录只会请求 mp.weixin.qq.com".into(),
            ));
        }
        let mut request = self
            .client
            .get(parsed)
            .header(reqwest::header::USER_AGENT, user_agent)
            .header(reqwest::header::ACCEPT, "application/json, text/plain, */*")
            .header(reqwest::header::REFERER, referer);
        if !cookie.is_empty() {
            request = request.header(reqwest::header::COOKIE, cookie);
        }
        let response = request
            .send()
            .await
            .map_err(|err| EngineError::Fetch(redact_secrets(&err.to_string())))?;
        let status = response.status();
        let final_host = response.url().host_str().map(str::to_string);
        if !is_mp_host(final_host.as_deref()) {
            return Err(EngineError::Message(
                "微信把目录请求转到了登录或验证页。请重新登录后再保存会话。ArticleBook 不会绕过验证。"
                    .into(),
            ));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|err| EngineError::Fetch(redact_secrets(&err.to_string())))?;
        if !status.is_success() {
            return Err(EngineError::Fetch(format!(
                "历史目录返回 HTTP {}",
                status.as_u16()
            )));
        }
        Ok(String::from_utf8_lossy(&bytes)
            .trim_start_matches('\u{feff}')
            .to_string())
    }
}

pub fn session_file(data_dir: &Path) -> PathBuf {
    data_dir.join(SESSION_FILE_NAME)
}

pub fn describe_wechat_page(url: &str) -> PageHint {
    match session_from_page_url(url, "", "") {
        Ok(session) => PageHint {
            usable: true,
            message: format!(
                "这是{}页。点「我已打开历史消息 · 保存」。",
                session.kind.label()
            ),
            kind: session.kind.as_str().to_string(),
            biz: session.biz,
        },
        Err(err) => PageHint {
            usable: false,
            message: err.to_string(),
            kind: String::new(),
            biz: String::new(),
        },
    }
}

pub fn session_from_page_url(
    url: &str,
    cookie: &str,
    account_name: &str,
) -> Result<WechatWebSession, EngineError> {
    reject_cookie_injection(cookie)?;
    let parsed = Url::parse(url.trim())
        .map_err(|_| EngineError::Message("请粘贴浏览器地址栏里的完整 https 链接。".into()))?;
    if parsed.scheme() != "https" && parsed.scheme() != "http" {
        return Err(EngineError::Message(
            "只接受微信网页的 http(s) 地址。".into(),
        ));
    }
    if !is_mp_host(parsed.host_str()) {
        return Err(EngineError::Message(
            "请粘贴 mp.weixin.qq.com 上的历史消息页或公众平台图文列表。".into(),
        ));
    }
    let path = parsed.path();
    if path == "/s" || path.starts_with("/s/") {
        return Err(EngineError::Message(
            "这是单篇文章，不是目录。请打开公众号的「历史消息」，或公众平台里的图文素材列表，再复制地址栏。"
                .into(),
        ));
    }
    let biz = query_value(&parsed, "__biz");
    let token = query_value(&parsed, "token");
    let kind = if path.contains("profile_ext") || (!biz.is_empty() && token.is_empty()) {
        if biz.is_empty() {
            return Err(EngineError::Message(
                "地址里没有 __biz。请打开某个公众号的历史消息页后再复制。".into(),
            ));
        }
        HistoryKind::ReaderHistory
    } else if path.contains("appmsg") || path.contains("cgi-bin") {
        if token.is_empty() {
            return Err(EngineError::Message(
                "公众平台地址里没有 token。请先登录，打开图文素材列表后再复制地址栏。".into(),
            ));
        }
        HistoryKind::OwnerConsole
    } else if !biz.is_empty() {
        HistoryKind::ReaderHistory
    } else {
        return Err(EngineError::Message(
            "这还不是历史目录。登录后打开公众号「历史消息」，或公众平台的图文列表，再复制地址栏。"
                .into(),
        ));
    };
    let mut source = parsed.clone();
    let _ = source.set_scheme("https");
    Ok(WechatWebSession {
        version: 1,
        kind,
        account_name: account_name.trim().to_string(),
        biz,
        uin: query_value(&parsed, "uin"),
        key: query_value(&parsed, "key"),
        pass_ticket: query_value(&parsed, "pass_ticket"),
        token,
        cookie: cookie.trim().to_string(),
        source_url: source.to_string(),
        captured_at: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
    })
}

/// Build a session from a page URL. An empty cookie keeps the previous one.
/// `clear_cookie` drops it even when a new cookie was pasted.
pub fn prepare_session(
    page_url: &str,
    cookie: &str,
    account_name: &str,
    previous: Option<&WechatWebSession>,
    clear_cookie: bool,
) -> Result<WechatWebSession, EngineError> {
    let incoming = if clear_cookie { "" } else { cookie };
    let mut session = session_from_page_url(page_url, incoming, account_name)?;
    if let Some(previous) = previous {
        if session.account_name.trim().is_empty() {
            session.account_name = previous.account_name.clone();
        }
        if session.cookie.is_empty() && !clear_cookie {
            session.cookie = previous.cookie.clone();
        }
    }
    if clear_cookie {
        session.cookie.clear();
    }
    reject_cookie_injection(&session.cookie)?;
    Ok(session)
}

pub fn save_session(path: &Path, session: &WechatWebSession) -> Result<(), EngineError> {
    reject_cookie_injection(&session.cookie)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_vec_pretty(session)
        .map_err(|err| EngineError::Message(format!("无法保存会话：{err}")))?;
    write_private(path, &json)
}

pub fn load_session(path: &Path) -> Result<WechatWebSession, EngineError> {
    let bytes = std::fs::read(path).map_err(|err| EngineError::reading(path, err))?;
    let session: WechatWebSession = serde_json::from_slice(&bytes)
        .map_err(|_| EngineError::Message("会话文件无法读取。请在界面里重新保存一次。".into()))?;
    if session.version == 0 {
        return Err(EngineError::Message(
            "会话文件版本不对。请重新保存。".into(),
        ));
    }
    reject_cookie_injection(&session.cookie)?;
    Ok(session)
}

pub fn parse_history_document(body: &str) -> Result<Vec<HistoryEntry>, EngineError> {
    let page = parse_history_page(body)?;
    Ok(page.entries)
}

pub async fn collect_history(
    client: &dyn HistoryClient,
    session: &WechatWebSession,
    limit: usize,
    delay: Duration,
) -> Result<Vec<HistoryEntry>, EngineError> {
    reject_cookie_injection(&session.cookie)?;
    // `0` matches the public `history_limit` sentinel: no cap besides the safety ceiling.
    let limit = resolve_history_limit(limit);
    let pace = Pace::new(delay);
    let mut offset = 0usize;
    let mut entries = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for _ in 0..SAFETY_MAX_PAGES {
        if entries.len() >= limit {
            break;
        }
        pace.wait().await;
        let url = history_request_url(session, offset, PAGE_SIZE)?;
        let referer = history_referer(session)?;
        let body = client
            .get_text(&url, session.cookie.trim(), USER_AGENT, &referer)
            .await?;
        let page = parse_history_page(&body)?;
        if page.entries.is_empty() {
            break;
        }
        let mut added = 0usize;
        for entry in page.entries {
            if entries.len() >= limit {
                break;
            }
            if seen.insert(entry.url.clone()) {
                entries.push(entry);
                added += 1;
            }
        }
        if entries.len() >= limit || added == 0 || !page.can_continue {
            break;
        }
        let step = if page.step > 0 { page.step } else { PAGE_SIZE };
        let next = offset.saturating_add(step);
        if next <= offset {
            break;
        }
        offset = next;
    }
    if entries.is_empty() {
        return Err(EngineError::Message(
            "这个会话没有返回文章。请确认打开的是你有权阅读的历史消息或图文列表，并且会话还没过期。"
                .into(),
        ));
    }
    Ok(entries)
}

/// Resolve a caller-supplied article cap.
///
/// `0` means all readable history, up to [`SAFETY_MAX_ARTICLES`]. Any other value
/// is "only the most recent N", also clamped to that ceiling.
pub fn history_limit_or_default(limit: u32) -> usize {
    resolve_history_limit(limit as usize)
}

fn resolve_history_limit(limit: usize) -> usize {
    if limit == 0 {
        SAFETY_MAX_ARTICLES
    } else {
        limit.clamp(1, SAFETY_MAX_ARTICLES)
    }
}

struct HistoryPage {
    entries: Vec<HistoryEntry>,
    can_continue: bool,
    step: usize,
}

fn parse_history_page(body: &str) -> Result<HistoryPage, EngineError> {
    let trimmed = body.trim().trim_start_matches('\u{feff}');
    if looks_like_html(trimmed) {
        return Err(login_wall_error(trimmed));
    }
    let value: Value = serde_json::from_str(trimmed).map_err(|_| {
        EngineError::Message("历史目录不是 JSON。会话可能已失效，请重新登录后再保存。".into())
    })?;
    if let Some(code) = json_i64(value.get("ret")).or_else(|| {
        value
            .get("base_resp")
            .and_then(|item| json_i64(item.get("ret")))
    }) {
        if code != 0 {
            let reason = json_string(value.get("errmsg"))
                .or_else(|| {
                    value
                        .get("base_resp")
                        .and_then(|item| json_string(item.get("err_msg")))
                })
                .unwrap_or_else(|| format!("ret={code}"));
            return Err(EngineError::Message(format!(
                "微信拒绝了目录请求（{reason}）。请重新登录后再保存会话。ArticleBook 不会绕过验证。"
            )));
        }
    }
    if let Some(list) = value.get("app_msg_list").and_then(Value::as_array) {
        let entries = entries_from_owner_list(list);
        let step = list.len();
        // A short page is the end of the owner list. An explicit flag wins when present.
        let continue_flag = explicit_continue(&value).unwrap_or(list.len() >= PAGE_SIZE);
        return Ok(HistoryPage {
            entries,
            can_continue: continue_flag,
            step,
        });
    }
    let general = match value.get("general_msg_list") {
        Some(Value::String(text)) => serde_json::from_str::<Value>(text)
            .map_err(|_| EngineError::Message("历史目录里的 general_msg_list 无法解析。".into()))?,
        Some(other) => other.clone(),
        None => {
            return Err(EngineError::Message(
                "响应里没有文章列表。请确认这是历史消息或图文素材接口。".into(),
            ));
        }
    };
    let list = general
        .get("list")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let entries = entries_from_reader_list(&list);
    let step = json_i64(value.get("msg_count"))
        .map(|n| n.max(0) as usize)
        .filter(|n| *n > 0)
        .unwrap_or(list.len());
    let can_continue = explicit_continue(&value).unwrap_or(false);
    Ok(HistoryPage {
        entries,
        can_continue,
        step,
    })
}

fn entries_from_reader_list(list: &[Value]) -> Vec<HistoryEntry> {
    let mut entries = Vec::new();
    for item in list {
        if let Some(ext) = item.get("app_msg_ext_info") {
            push_article(
                &mut entries,
                json_string(ext.get("content_url")).as_deref(),
                json_string(ext.get("title")).as_deref(),
            );
            if let Some(multi) = ext.get("multi_app_msg_item_list").and_then(Value::as_array) {
                for child in multi {
                    push_article(
                        &mut entries,
                        json_string(child.get("content_url")).as_deref(),
                        json_string(child.get("title")).as_deref(),
                    );
                }
            }
        }
    }
    entries
}

fn entries_from_owner_list(list: &[Value]) -> Vec<HistoryEntry> {
    let mut entries = Vec::new();
    for item in list {
        let link = json_string(item.get("link")).or_else(|| json_string(item.get("content_url")));
        push_article(
            &mut entries,
            link.as_deref(),
            json_string(item.get("title")).as_deref(),
        );
    }
    entries
}

fn push_article(entries: &mut Vec<HistoryEntry>, raw_url: Option<&str>, title: Option<&str>) {
    let Some(raw_url) = raw_url else {
        return;
    };
    let raw_url = unescape_basic(raw_url.trim());
    if raw_url.is_empty() {
        return;
    }
    let absolute = if let Some(rest) = raw_url.strip_prefix("//") {
        format!("https://{rest}")
    } else if raw_url.starts_with("http://mp.weixin.qq.com") {
        format!("https://{}", &raw_url["http://".len()..])
    } else {
        raw_url
    };
    let Ok(url) = normalize_url(&absolute, &NormalizeOptions::default()) else {
        return;
    };
    if !url.contains("mp.weixin.qq.com") {
        return;
    }
    entries.push(HistoryEntry {
        url,
        title: title.unwrap_or("").trim().to_string(),
    });
}

fn history_request_url(
    session: &WechatWebSession,
    offset: usize,
    count: usize,
) -> Result<String, EngineError> {
    let mut url = match session.kind {
        HistoryKind::ReaderHistory => {
            if session.biz.trim().is_empty() {
                return Err(EngineError::Message(
                    "会话里没有 __biz，无法拉取历史消息。".into(),
                ));
            }
            let mut url = Url::parse("https://mp.weixin.qq.com/mp/profile_ext")
                .map_err(|err| EngineError::BadUrl(err.to_string()))?;
            {
                let mut pairs = url.query_pairs_mut();
                pairs.append_pair("action", "getmsg");
                pairs.append_pair("__biz", session.biz.trim());
                pairs.append_pair("f", "json");
                pairs.append_pair("offset", &offset.to_string());
                pairs.append_pair("count", &count.to_string());
                append_if(&mut pairs, "uin", &session.uin);
                append_if(&mut pairs, "key", &session.key);
                append_if(&mut pairs, "pass_ticket", &session.pass_ticket);
            }
            url
        }
        HistoryKind::OwnerConsole => {
            if session.token.trim().is_empty() {
                return Err(EngineError::Message(
                    "会话里没有公众平台 token，无法拉取图文列表。".into(),
                ));
            }
            let mut url = Url::parse("https://mp.weixin.qq.com/cgi-bin/appmsg")
                .map_err(|err| EngineError::BadUrl(err.to_string()))?;
            {
                let mut pairs = url.query_pairs_mut();
                pairs.append_pair("action", "list_ex");
                pairs.append_pair("begin", &offset.to_string());
                pairs.append_pair("count", &count.to_string());
                pairs.append_pair("type", "9");
                pairs.append_pair("lang", "zh_CN");
                pairs.append_pair("f", "json");
                pairs.append_pair("token", session.token.trim());
            }
            url
        }
    };
    let _ = url.set_scheme("https");
    Ok(url.to_string())
}

fn history_referer(session: &WechatWebSession) -> Result<String, EngineError> {
    let mut url = match session.kind {
        HistoryKind::ReaderHistory => {
            let mut url = Url::parse("https://mp.weixin.qq.com/mp/profile_ext")
                .map_err(|err| EngineError::BadUrl(err.to_string()))?;
            url.query_pairs_mut()
                .append_pair("action", "home")
                .append_pair("__biz", session.biz.trim());
            url
        }
        HistoryKind::OwnerConsole => {
            let mut url = Url::parse("https://mp.weixin.qq.com/cgi-bin/appmsg")
                .map_err(|err| EngineError::BadUrl(err.to_string()))?;
            url.query_pairs_mut()
                .append_pair("action", "list_ex")
                .append_pair("type", "9")
                .append_pair("lang", "zh_CN")
                .append_pair("token", session.token.trim());
            url
        }
    };
    let _ = url.set_scheme("https");
    Ok(url.to_string())
}

fn append_if<T: Target>(pairs: &mut url::form_urlencoded::Serializer<T>, key: &str, value: &str) {
    let value = value.trim();
    if !value.is_empty() {
        pairs.append_pair(key, value);
    }
}

fn query_value(url: &Url, key: &str) -> String {
    url.query_pairs()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.into_owned())
        .unwrap_or_default()
}

fn is_mp_host(host: Option<&str>) -> bool {
    matches!(host, Some("mp.weixin.qq.com"))
}

fn looks_like_html(body: &str) -> bool {
    let start = body.trim_start();
    start.starts_with('<')
}

fn login_wall_error(body: &str) -> EngineError {
    if body.contains("环境异常") || body.contains("去验证") {
        EngineError::Message(
            "微信返回了验证页。ArticleBook 不会绕过验证。请在浏览器里自己完成验证后，再保存新的会话。"
                .into(),
        )
    } else {
        EngineError::Message(
            "微信返回了网页而不是目录。会话可能已失效，请重新登录后再保存。".into(),
        )
    }
}

fn reject_cookie_injection(cookie: &str) -> Result<(), EngineError> {
    if cookie.contains('\n') || cookie.contains('\r') {
        return Err(EngineError::Message(
            "Cookie 里不能换行。请只粘贴请求头里的那一行。".into(),
        ));
    }
    Ok(())
}

fn unescape_basic(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

fn json_string(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_string)
}

fn json_i64(value: Option<&Value>) -> Option<i64> {
    value.and_then(|item| {
        item.as_i64().or_else(|| {
            item.as_str()
                .and_then(|text| text.trim().parse::<i64>().ok())
        })
    })
}

/// `can_msg_continue` is the reader history flag. `can_continue` is accepted for the same meaning.
fn explicit_continue(value: &Value) -> Option<bool> {
    for key in ["can_msg_continue", "can_continue"] {
        if value.get(key).is_some() {
            return Some(json_flag(value.get(key)));
        }
    }
    None
}

fn json_flag(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(flag)) => *flag,
        Some(other) => json_i64(Some(other)).unwrap_or(0) != 0,
        None => false,
    }
}

fn redact_secrets(text: &str) -> String {
    let mut out = text.to_string();
    for key in ["key", "pass_ticket", "appmsg_token"] {
        out = redact_query_key(&out, key);
    }
    out
}

fn redact_query_key(text: &str, key: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let pattern = format!("{key}=");
    while let Some(index) = rest.find(&pattern) {
        out.push_str(&rest[..index + pattern.len()]);
        out.push_str("***");
        rest = &rest[index + pattern.len()..];
        if let Some(end) = rest.find(['&', ' ', '"', '\'']) {
            rest = &rest[end..];
        } else {
            rest = "";
        }
    }
    out.push_str(rest);
    out
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), EngineError> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::EngineError;
    use async_trait::async_trait;
    use std::sync::Mutex;
    use std::time::Duration;

    #[test]
    fn parses_reader_page_url() {
        let session = session_from_page_url(
            "https://mp.weixin.qq.com/mp/profile_ext?action=home&__biz=MzFixture%3D%3D&uin=777&key=abc&pass_ticket=pt&scene=124",
            "wap_sid2=test",
            "示例号",
        )
        .unwrap();
        assert_eq!(session.kind, HistoryKind::ReaderHistory);
        assert_eq!(session.biz, "MzFixture==");
        assert_eq!(session.uin, "777");
        assert_eq!(session.key, "abc");
        assert_eq!(session.pass_ticket, "pt");
        assert_eq!(session.cookie, "wap_sid2=test");
        assert_eq!(session.account_name, "示例号");
    }

    #[test]
    fn parses_owner_console_url_and_rejects_article_or_other_host() {
        let session = session_from_page_url(
            "https://mp.weixin.qq.com/cgi-bin/appmsg?action=list_ex&type=9&token=TOK&lang=zh_CN",
            "",
            "",
        )
        .unwrap();
        assert_eq!(session.kind, HistoryKind::OwnerConsole);
        assert_eq!(session.token, "TOK");
        let article = session_from_page_url("https://mp.weixin.qq.com/s/demo", "", "").unwrap_err();
        assert!(article.to_string().contains("单篇"), "{article}");
        let other = session_from_page_url("https://example.com/mp/profile_ext?__biz=Mz", "", "")
            .unwrap_err();
        assert!(other.to_string().contains("mp.weixin.qq.com"), "{other}");
    }

    #[test]
    fn parses_getmsg_fixture_and_skips_plain_text() {
        let body = include_str!("../../../fixtures/wechat_history_getmsg.json");
        let entries = parse_history_document(body).unwrap();
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.title.as_str())
                .collect::<Vec<_>>(),
            vec!["把长文收成一本小书", "第二篇"]
        );
        assert!(entries.iter().all(|entry| !entry.url.contains("scene=")));
        assert!(entries
            .iter()
            .all(|entry| entry.url.contains("__biz=MzFixture")));
    }

    #[test]
    fn parses_html_escaped_second_page_and_owner_list() {
        let page2 = include_str!("../../../fixtures/wechat_history_getmsg_page2.json");
        let entries = parse_history_document(page2).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].url.starts_with("https://mp.weixin.qq.com/s?"));
        assert!(entries[0].url.contains("sn=ccc"));
        let owner = include_str!("../../../fixtures/wechat_history_appmsg.json");
        let entries = parse_history_document(owner).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries[1].url.contains("/s/short-owner"));
    }

    #[test]
    fn expired_json_and_verify_html_fail_closed() {
        let expired =
            parse_history_document(r#"{"ret":200003,"errmsg":"登录态失效"}"#).unwrap_err();
        assert!(expired.to_string().contains("不会绕过验证"), "{expired}");
        let html = parse_history_document("<html>环境异常 去验证</html>").unwrap_err();
        assert!(html.to_string().contains("不会绕过验证"), "{html}");
    }

    #[test]
    fn request_url_stays_on_mp_and_cookie_cannot_break_headers() {
        let session = session_from_page_url(
            "https://mp.weixin.qq.com/mp/profile_ext?action=home&__biz=MzA&key=abc&pass_ticket=pt",
            "",
            "",
        )
        .unwrap();
        let url = history_request_url(&session, 10, 10).unwrap();
        assert!(url.starts_with("https://mp.weixin.qq.com/mp/profile_ext?"));
        assert!(url.contains("offset=10"));
        assert!(url.contains("action=getmsg"));
        assert!(!url.contains("MicroMessenger"));
        let bad = session_from_page_url(
            "https://mp.weixin.qq.com/mp/profile_ext?__biz=MzA",
            "a=b\nCookie: evil",
            "",
        );
        assert!(bad.is_err());
    }

    #[test]
    fn empty_cookie_keeps_saved_cookie_until_cleared() {
        let first = session_from_page_url(
            "https://mp.weixin.qq.com/mp/profile_ext?action=home&__biz=MzA&key=abc",
            "wap_sid2=test",
            "示例号",
        )
        .unwrap();
        let kept = prepare_session(
            "https://mp.weixin.qq.com/mp/profile_ext?action=home&__biz=MzA&key=next",
            "  ",
            "",
            Some(&first),
            false,
        )
        .unwrap();
        assert_eq!(kept.cookie, "wap_sid2=test");
        assert_eq!(kept.key, "next");
        assert_eq!(kept.account_name, "示例号");
        let replaced = prepare_session(
            "https://mp.weixin.qq.com/mp/profile_ext?action=home&__biz=MzA",
            "wap_sid2=new",
            "",
            Some(&first),
            false,
        )
        .unwrap();
        assert_eq!(replaced.cookie, "wap_sid2=new");
        let cleared = prepare_session(
            "https://mp.weixin.qq.com/mp/profile_ext?action=home&__biz=MzA",
            "wap_sid2=new",
            "另一号",
            Some(&first),
            true,
        )
        .unwrap();
        assert!(cleared.cookie.is_empty());
        assert_eq!(cleared.account_name, "另一号");
    }

    #[test]
    fn zero_history_limit_means_all_up_to_the_safety_ceiling() {
        assert_eq!(history_limit_or_default(0), SAFETY_MAX_ARTICLES);
        assert_eq!(history_limit_or_default(40), 40);
        assert_eq!(history_limit_or_default(250), 250);
        assert_eq!(
            history_limit_or_default(u32::MAX),
            SAFETY_MAX_ARTICLES,
            "a recent-N cap cannot exceed the safety ceiling"
        );
        assert!(SAFETY_MAX_ARTICLES > 200);
        assert!(SAFETY_MAX_PAGES >= SAFETY_MAX_ARTICLES / 10);
    }

    fn reader_session() -> WechatWebSession {
        session_from_page_url(
            "https://mp.weixin.qq.com/mp/profile_ext?action=home&__biz=MzFixture&key=abc",
            "wap_sid2=test",
            "示例号",
        )
        .unwrap()
    }

    fn owner_session() -> WechatWebSession {
        session_from_page_url(
            "https://mp.weixin.qq.com/cgi-bin/appmsg?action=list_ex&type=9&token=TOK&lang=zh_CN",
            "slave_sid=test",
            "我的号",
        )
        .unwrap()
    }

    fn reader_page(start: usize, count: usize, can_continue: bool) -> String {
        let list: Vec<_> = (0..count)
            .map(|i| {
                let n = start + i;
                serde_json::json!({
                    "comm_msg_info": {"type": 49, "id": n},
                    "app_msg_ext_info": {
                        "title": format!("文章{n}"),
                        "content_url": format!(
                            "https://mp.weixin.qq.com/s?__biz=MzFixture&mid={n}&idx=1&sn=s{n}"
                        )
                    }
                })
            })
            .collect::<Vec<_>>();
        let general = serde_json::json!({ "list": list }).to_string();
        serde_json::json!({
            "ret": 0,
            "errmsg": "ok",
            "msg_count": count,
            "can_msg_continue": if can_continue { 1 } else { 0 },
            "general_msg_list": general
        })
        .to_string()
    }

    fn owner_page(start: usize, count: usize, explicit_stop: bool) -> String {
        let list: Vec<_> = (0..count)
            .map(|i| {
                let n = start + i;
                serde_json::json!({
                    "title": format!("后台{n}"),
                    "link": format!(
                        "https://mp.weixin.qq.com/s?__biz=MzOwner&mid={n}&idx=1&sn=o{n}"
                    )
                })
            })
            .collect();
        let mut value = serde_json::json!({
            "base_resp": {"ret": 0, "err_msg": "ok"},
            "app_msg_list": list
        });
        if explicit_stop {
            value["can_continue"] = serde_json::json!(0);
        }
        value.to_string()
    }

    struct ScriptedHistory {
        pages: Mutex<Vec<String>>,
        calls: Mutex<Vec<String>>,
    }

    impl ScriptedHistory {
        fn new(pages: Vec<String>) -> Self {
            Self {
                pages: Mutex::new(pages),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> usize {
            self.calls.lock().expect("calls").len()
        }
    }

    #[async_trait]
    impl HistoryClient for ScriptedHistory {
        async fn get_text(
            &self,
            url: &str,
            _cookie: &str,
            user_agent: &str,
            referer: &str,
        ) -> Result<String, EngineError> {
            assert!(url.starts_with("https://mp.weixin.qq.com/"), "{url}");
            assert!(!user_agent.contains("MicroMessenger"));
            assert!(referer.starts_with("https://mp.weixin.qq.com/"));
            self.calls.lock().expect("calls").push(url.to_string());
            let mut pages = self.pages.lock().expect("pages");
            if pages.is_empty() {
                return Err(EngineError::Fetch("没有更多目录页".into()));
            }
            Ok(pages.remove(0))
        }
    }

    struct EndlessHistory {
        per_page: usize,
        calls: std::sync::atomic::AtomicUsize,
    }

    #[async_trait]
    impl HistoryClient for EndlessHistory {
        async fn get_text(
            &self,
            url: &str,
            _cookie: &str,
            user_agent: &str,
            _referer: &str,
        ) -> Result<String, EngineError> {
            assert!(url.starts_with("https://mp.weixin.qq.com/"));
            assert!(!user_agent.contains("MicroMessenger"));
            let page = self
                .calls
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let start = page * self.per_page;
            Ok(reader_page(start, self.per_page, true))
        }
    }

    #[tokio::test]
    async fn zero_limit_keeps_paging_until_the_api_says_stop() {
        let pages = 25;
        let per_page = 10;
        let bodies = (0..pages)
            .map(|page| reader_page(page * per_page, per_page, page + 1 != pages))
            .collect();
        let client = ScriptedHistory::new(bodies);
        let entries = collect_history(
            &client,
            &reader_session(),
            history_limit_or_default(0),
            Duration::ZERO,
        )
        .await
        .unwrap();
        assert_eq!(entries.len(), 250);
        assert!(entries.len() > 200, "全部 must not stop at the old 200 cap");
        assert_eq!(client.calls(), pages);
        assert!(entries.last().unwrap().url.contains("mid=249"));
    }

    #[tokio::test]
    async fn recent_limit_above_two_hundred_is_honored_and_a_small_limit_stops() {
        let bodies = (0..30)
            .map(|page| reader_page(page * 10, 10, true))
            .collect();
        let client = ScriptedHistory::new(bodies);
        let entries = collect_history(&client, &reader_session(), 250, Duration::ZERO)
            .await
            .unwrap();
        assert_eq!(entries.len(), 250);
        assert_eq!(client.calls(), 25, "200 would have stopped after 20 pages");

        let bodies = (0..10)
            .map(|page| reader_page(page * 10, 10, true))
            .collect();
        let client = ScriptedHistory::new(bodies);
        let entries = collect_history(&client, &reader_session(), 15, Duration::ZERO)
            .await
            .unwrap();
        assert_eq!(entries.len(), 15);
        assert_eq!(client.calls(), 2);
    }

    #[tokio::test]
    async fn empty_page_or_explicit_stop_ends_the_pull() {
        let client = ScriptedHistory::new(vec![
            reader_page(0, 10, true),
            reader_page(1000, 0, true),
            reader_page(2000, 10, true),
        ]);
        let entries = collect_history(&client, &reader_session(), 0, Duration::ZERO)
            .await
            .unwrap();
        assert_eq!(entries.len(), 10);
        assert_eq!(client.calls(), 2, "an empty page must not request another");

        let client = ScriptedHistory::new(vec![owner_page(0, 10, true), owner_page(10, 10, false)]);
        let entries = collect_history(&client, &owner_session(), 0, Duration::ZERO)
            .await
            .unwrap();
        assert_eq!(entries.len(), 10);
        assert_eq!(
            client.calls(),
            1,
            "explicit can_continue=0 stops a full page"
        );

        let client = ScriptedHistory::new(vec![owner_page(0, 10, false), owner_page(10, 3, false)]);
        let entries = collect_history(&client, &owner_session(), 0, Duration::ZERO)
            .await
            .unwrap();
        assert_eq!(entries.len(), 13);
        assert_eq!(client.calls(), 2);
    }

    #[tokio::test]
    async fn endless_history_stops_on_the_safety_ceiling() {
        let client = EndlessHistory {
            per_page: 10,
            calls: std::sync::atomic::AtomicUsize::new(0),
        };
        let entries = collect_history(&client, &reader_session(), 0, Duration::ZERO)
            .await
            .unwrap();
        assert_eq!(entries.len(), SAFETY_MAX_ARTICLES);
        assert_eq!(
            client.calls.load(std::sync::atomic::Ordering::Relaxed),
            SAFETY_MAX_ARTICLES / 10
        );
        assert!(client.calls.load(std::sync::atomic::Ordering::Relaxed) <= SAFETY_MAX_PAGES);

        let client = EndlessHistory {
            per_page: 1,
            calls: std::sync::atomic::AtomicUsize::new(0),
        };
        let entries = collect_history(&client, &reader_session(), 0, Duration::ZERO)
            .await
            .unwrap();
        assert_eq!(entries.len(), SAFETY_MAX_PAGES);
        assert_eq!(
            client.calls.load(std::sync::atomic::Ordering::Relaxed),
            SAFETY_MAX_PAGES
        );
        assert!(entries.len() > 200);
        assert!(entries.len() < SAFETY_MAX_ARTICLES);
    }
}

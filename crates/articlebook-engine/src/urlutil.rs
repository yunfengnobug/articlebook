use crate::error::EngineError;
use serde::Serialize;
use std::collections::HashSet;
use url::Url;

/// How list URLs are canonicalized before fetch and cache lookup.
#[derive(Debug, Clone)]
pub struct NormalizeOptions {
    pub strip_tracking: bool,
    pub extra_strip: Vec<String>,
    pub keep_fragment: bool,
}

impl Default for NormalizeOptions {
    fn default() -> Self {
        Self {
            strip_tracking: true,
            extra_strip: Vec::new(),
            keep_fragment: false,
        }
    }
}

/// Result of splitting a paste or a links file.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedUrls {
    pub urls: Vec<String>,
    pub invalid: Vec<String>,
    pub duplicates: usize,
}

const GLOBAL_TRACKING: &[&str] = &[
    "fbclid",
    "gclid",
    "gclsrc",
    "dclid",
    "gbraid",
    "wbraid",
    "mc_cid",
    "mc_eid",
    "igshid",
    "igsh",
    "_ga",
    "_gl",
    "msclkid",
    "yclid",
    "twclid",
    "spm",
    "scm",
    "mkt_tok",
    "_hsenc",
    "_hsmi",
    "vero_id",
    "oly_anon_id",
    "oly_enc_id",
    "si",
    "s_kwcid",
    "ref_src",
    "trk",
    "trkcampaign",
];

/// Query keys that change between shares but do not identify a WeChat article.
/// `__biz`, `mid`, `idx`, `sn` and `itemidx` are kept.
const WECHAT_TRACKING: &[&str] = &[
    "scene",
    "subscene",
    "clicktime",
    "enterid",
    "ascene",
    "devicetype",
    "version",
    "nettype",
    "lang",
    "wx_header",
    "sharer_shareid",
    "sharer_sharetime",
    "sharer_share_time",
    "exportkey",
    "pass_ticket",
    "poc_token",
    "color_scheme",
    "fontscale",
    "srcid",
    "mpshare",
    "click_id",
    "chksm",
    "source",
    "share",
    "is_only_read",
    "is_need_ad",
    "sessionid",
    "from",
    "rank",
    "nwr_flag",
    "appinstall",
    "user_article_info",
    "version_code",
    "wx_fmt",
    "forceh5",
    "key",
    "uin",
    "abtest_cookie",
    "nsukey",
];

pub fn is_wechat_host(host: &str) -> bool {
    let host = host.trim_start_matches("www.").to_ascii_lowercase();
    host == "mp.weixin.qq.com" || host.ends_with(".mp.weixin.qq.com")
}

pub fn url_is_wechat(url: &str) -> bool {
    Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| is_wechat_host(h)))
        .unwrap_or(false)
}

/// Canonical form used for dedupe and the on-disk cache key.
pub fn normalize_url(input: &str, opts: &NormalizeOptions) -> Result<String, EngineError> {
    let trimmed = trim_url_tail(
        input
            .trim()
            .trim_matches(|c| matches!(c, '<' | '>' | '"' | '\'')),
    );
    if trimmed.is_empty() {
        return Err(EngineError::BadUrl("空链接".into()));
    }
    let candidate = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };
    let mut url =
        Url::parse(&candidate).map_err(|err| EngineError::BadUrl(format!("{trimmed}（{err}）")))?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(EngineError::BadUrl(format!(
            "不支持的协议 {}",
            url.scheme()
        )));
    }
    if url.host_str().is_none() {
        return Err(EngineError::BadUrl(trimmed.to_string()));
    }
    if !opts.keep_fragment {
        url.set_fragment(None);
    }
    let wechat = url.host_str().map(is_wechat_host).unwrap_or(false);
    let mut pairs: Vec<(String, String)> = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .filter(|(k, _)| !opts.strip_tracking || !is_tracking(k, wechat, &opts.extra_strip))
        .collect();
    pairs.sort();
    if pairs.is_empty() {
        url.set_query(None);
    } else {
        let mut query = url.query_pairs_mut();
        query.clear();
        for (k, v) in &pairs {
            query.append_pair(k, v);
        }
        drop(query);
    }
    let path = url.path().to_string();
    let trimmed_path = path.trim_end_matches('/');
    if !trimmed_path.is_empty() && trimmed_path != path {
        url.set_path(trimmed_path);
    }
    Ok(url.to_string())
}

/// Pull http(s) URLs out of messy pasted text. Blank lines and `#` comments are ignored.
pub fn parse_url_list(text: &str, opts: &NormalizeOptions) -> ParsedUrls {
    let mut urls = Vec::new();
    let mut invalid = Vec::new();
    let mut duplicates = 0;
    let mut seen = HashSet::new();
    for raw_line in text.lines() {
        let line = raw_line.trim().trim_end_matches('\r');
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let found = extract_raw_urls(line);
        if found.is_empty() {
            invalid.push(line.to_string());
            continue;
        }
        for raw in found {
            match normalize_url(&raw, opts) {
                Ok(url) => {
                    if !seen.insert(url.clone()) {
                        duplicates += 1;
                    } else {
                        urls.push(url);
                    }
                }
                Err(_) => invalid.push(raw),
            }
        }
    }
    ParsedUrls {
        urls,
        invalid,
        duplicates,
    }
}

fn extract_raw_urls(line: &str) -> Vec<String> {
    let mut found = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let rest = &line[i..];
        let start = rest
            .find("https://")
            .map(|p| (p, "https://"))
            .or_else(|| rest.find("http://").map(|p| (p, "http://")));
        let Some((pos, scheme)) = start else {
            break;
        };
        let abs = i + pos;
        let after = abs + scheme.len();
        let tail = &line[after..];
        let end_rel = tail
            .find(|c: char| {
                c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\'' | '“' | '”' | '‘' | '’')
            })
            .unwrap_or(tail.len());
        let raw = trim_url_tail(&line[abs..after + end_rel]).to_string();
        if !raw.is_empty() {
            found.push(raw);
        }
        i = after + end_rel;
        if i == abs {
            i += 1;
        }
    }
    if found.is_empty() && looks_schemeless(line) {
        found.push(trim_url_tail(line).to_string());
    }
    found
}

fn looks_schemeless(line: &str) -> bool {
    let t = trim_url_tail(line.trim());
    if t.contains(char::is_whitespace) || t.contains('@') {
        return false;
    }
    let host = t.split('/').next().unwrap_or("");
    host.contains('.') && !host.starts_with('.')
}

fn trim_url_tail(input: &str) -> &str {
    let mut s = input.trim();
    const TAIL: &[char] = &[
        '.', ',', ';', ':', '!', '?', '，', '。', '、', '）', ')', ']', '>', '》', '"', '\'', '”',
        '’',
    ];
    while let Some(last) = s.chars().last() {
        if TAIL.contains(&last) {
            s = &s[..s.len() - last.len_utf8()];
        } else {
            break;
        }
    }
    s
}

fn is_tracking(key: &str, wechat: bool, extra: &[String]) -> bool {
    let key = key.to_ascii_lowercase();
    if extra.iter().any(|item| item.eq_ignore_ascii_case(&key)) {
        return true;
    }
    if key.starts_with("utm_") {
        return true;
    }
    if GLOBAL_TRACKING.contains(&key.as_str()) {
        return true;
    }
    wechat && WECHAT_TRACKING.contains(&key.as_str())
}

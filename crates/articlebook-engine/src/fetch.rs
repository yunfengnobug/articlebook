use crate::error::EngineError;
use async_trait::async_trait;
use std::time::Duration;

/// Identifiable desktop UA. This is not a WeChat client impersonation.
pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 ArticleBook/0.1";

pub const MAX_RESPONSE_BYTES: usize = 15 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct Fetched {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
    pub final_url: String,
}

#[async_trait]
pub trait HttpClient: Send + Sync {
    async fn get(&self, url: &str) -> Result<Fetched, EngineError>;
}

pub struct ReqwestClient {
    client: reqwest::Client,
}

impl ReqwestClient {
    pub fn new(user_agent: &str, timeout: Duration) -> Result<Self, EngineError> {
        let client = reqwest::Client::builder()
            .user_agent(user_agent)
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::limited(8))
            .build()
            .map_err(|err| EngineError::Fetch(err.to_string()))?;
        Ok(Self { client })
    }
}

#[async_trait]
impl HttpClient for ReqwestClient {
    async fn get(&self, url: &str) -> Result<Fetched, EngineError> {
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|err| EngineError::Fetch(err.to_string()))?;
        if let Some(len) = response.content_length() {
            if len > MAX_RESPONSE_BYTES as u64 {
                return Err(EngineError::Fetch(format!("响应过大（{len} 字节）")));
            }
        }
        let status = response.status().as_u16();
        let final_url = response.url().to_string();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();
        let body = response
            .bytes()
            .await
            .map_err(|err| EngineError::Fetch(err.to_string()))?
            .to_vec();
        if body.len() > MAX_RESPONSE_BYTES {
            return Err(EngineError::Fetch(format!(
                "响应过大（{} 字节）",
                body.len()
            )));
        }
        Ok(Fetched {
            status,
            content_type,
            body,
            final_url,
        })
    }
}

pub fn decode_document(body: &[u8], content_type: &str) -> String {
    let label = charset_from_content_type(content_type).or_else(|| charset_from_html_prefix(body));
    let encoding = label
        .as_deref()
        .and_then(|label| encoding_rs::Encoding::for_label(label.as_bytes()))
        .unwrap_or(encoding_rs::UTF_8);
    let (text, _, _) = encoding.decode(body);
    text.trim_start_matches('\u{feff}').to_string()
}

fn charset_from_content_type(content_type: &str) -> Option<String> {
    for part in content_type.split(';').skip(1) {
        let part = part.trim();
        let value = part
            .split_once('=')
            .filter(|(key, _)| key.trim().eq_ignore_ascii_case("charset"))
            .map(|(_, value)| value.trim().trim_matches('"').trim_matches('\''))?;
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

fn charset_from_html_prefix(body: &[u8]) -> Option<String> {
    let end = body.len().min(4096);
    let prefix = String::from_utf8_lossy(&body[..end]).to_ascii_lowercase();
    let idx = prefix.find("charset=")?;
    let rest = prefix[idx + "charset=".len()..].trim_start();
    let rest = rest.trim_start_matches(['"', '\'']);
    let stop = rest
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_')
        .unwrap_or(rest.len());
    let label = &rest[..stop];
    if label.is_empty() {
        None
    } else {
        Some(label.to_string())
    }
}

pub fn decode_text_bytes(bytes: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.trim_start_matches('\u{feff}').to_string();
    }
    let (text, _, _) = encoding_rs::GBK.decode(bytes);
    text.into_owned()
}

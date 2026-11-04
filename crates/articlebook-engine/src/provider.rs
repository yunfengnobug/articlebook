use crate::error::EngineError;
use crate::fetch::decode_text_bytes;
use crate::history::{
    collect_history, history_limit_or_default, load_session, HistoryClient, ReqwestHistoryClient,
    SAFETY_MAX_ARTICLES,
};
use async_trait::async_trait;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// A way to turn an account identifier into a list of article URLs.
///
/// [`ManualProvider`] reads links the user already copied. [`WechatWebProvider`]
/// pages an official WeChat web history or publisher list with a session the user
/// saved after signing in themselves. WMPF is named so the UI can show it as unavailable.
#[async_trait]
pub trait AccountProvider: Send + Sync {
    fn id(&self) -> &str;
    fn display_name(&self) -> &str;
    async fn list_links(&self, request: &AccountRequest) -> Result<ProviderListing, EngineError>;
}

#[derive(Debug, Clone)]
pub struct AccountRequest {
    pub account_id: String,
    pub account_name: String,
    pub links_text: String,
    pub links_file: Option<PathBuf>,
    pub session_file: Option<PathBuf>,
    pub history_limit: u32,
}

#[derive(Debug, Clone)]
pub struct ProviderListing {
    pub account_name: String,
    pub urls_text: String,
    pub source_note: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    pub available: bool,
    pub note: String,
}

pub struct ManualProvider;

#[async_trait]
impl AccountProvider for ManualProvider {
    fn id(&self) -> &str {
        "manual"
    }

    fn display_name(&self) -> &str {
        "手动链接文件"
    }

    async fn list_links(&self, request: &AccountRequest) -> Result<ProviderListing, EngineError> {
        let text = if let Some(path) = request
            .links_file
            .as_ref()
            .filter(|path| !path.as_os_str().is_empty())
        {
            let bytes = std::fs::read(path).map_err(|err| EngineError::reading(path, err))?;
            decode_text_bytes(&bytes)
        } else {
            request.links_text.clone()
        };
        if text.trim().is_empty() {
            return Err(EngineError::Message(
                "请粘贴链接，或选择一个链接文件".into(),
            ));
        }
        let account_name = if request.account_name.trim().is_empty() {
            "未命名合集".into()
        } else {
            request.account_name.trim().to_string()
        };
        Ok(ProviderListing {
            account_name,
            urls_text: text,
            source_note: "目录来自手动导入的链接，没有访问微信后台。".into(),
        })
    }
}

pub struct WechatWebProvider {
    client: Arc<dyn HistoryClient>,
    page_delay: Duration,
}

impl WechatWebProvider {
    pub fn live() -> Result<Self, EngineError> {
        Ok(Self {
            client: Arc::new(ReqwestHistoryClient::new(Duration::from_secs(25))?),
            page_delay: Duration::from_millis(800),
        })
    }

    pub fn with_client(client: Arc<dyn HistoryClient>, page_delay: Duration) -> Self {
        Self { client, page_delay }
    }
}

#[async_trait]
impl AccountProvider for WechatWebProvider {
    fn id(&self) -> &str {
        "wechat-web"
    }

    fn display_name(&self) -> &str {
        "微信网页会话"
    }

    async fn list_links(&self, request: &AccountRequest) -> Result<ProviderListing, EngineError> {
        let path = request
            .session_file
            .as_ref()
            .ok_or_else(|| EngineError::Message("请先保存微信网页会话，再拉取历史目录。".into()))?;
        let session = load_session(path)?;
        let entries = collect_history(
            self.client.as_ref(),
            &session,
            history_limit_or_default(request.history_limit),
            self.page_delay,
        )
        .await?;
        let account_name = if !request.account_name.trim().is_empty() {
            request.account_name.trim().to_string()
        } else if !session.account_name.trim().is_empty() {
            session.account_name.clone()
        } else {
            "未命名公众号".into()
        };
        let urls_text = entries
            .iter()
            .map(|entry| entry.url.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let mut source_note = format!(
            "目录来自你保存的微信网页会话（{}），共 {} 篇。没有调用 WMPF 或 Frida。",
            session.kind.label(),
            entries.len()
        );
        if request.history_limit == 0 && entries.len() >= SAFETY_MAX_ARTICLES {
            source_note.push_str("已达到单次安全上限，更早的文章没有继续拉取。");
        }
        Ok(ProviderListing {
            account_name,
            urls_text,
            source_note,
        })
    }
}

struct UnavailableProvider {
    id: &'static str,
    name: &'static str,
    reason: &'static str,
}

#[async_trait]
impl AccountProvider for UnavailableProvider {
    fn id(&self) -> &str {
        self.id
    }

    fn display_name(&self) -> &str {
        self.name
    }

    async fn list_links(&self, _request: &AccountRequest) -> Result<ProviderListing, EngineError> {
        Err(EngineError::Message(self.reason.into()))
    }
}

pub fn describe_providers() -> Vec<ProviderInfo> {
    vec![
        ProviderInfo {
            id: "wechat-web".into(),
            name: "微信网页会话".into(),
            available: true,
            note: "用你自己登录后的微信网页，拉取一个你有权阅读的公众号的全部历史。单次有安全上限。不会调用 Frida 或 WMPF，也不会伪装微信客户端。".into(),
        },
        ProviderInfo {
            id: "manual".into(),
            name: "手动链接文件".into(),
            available: true,
            note: "导入你已经复制好的文章链接。不会登录微信。".into(),
        },
        ProviderInfo {
            id: "wmpf".into(),
            name: "微信登录 / WMPF".into(),
            available: false,
            note: "尚未实现。不会登录微信，也不会调用 Frida 或 WMPF。".into(),
        },
    ]
}

pub fn provider_by_id(id: &str) -> Result<Box<dyn AccountProvider>, EngineError> {
    match id {
        "manual" | "" => Ok(Box::new(ManualProvider)),
        "wechat-web" | "wechat-history" => Ok(Box::new(WechatWebProvider::live()?)),
        "wmpf" | "browser" | "wechat-login" => Ok(Box::new(UnavailableProvider {
            id: "wmpf",
            name: "微信登录 / WMPF",
            reason: "尚未实现。本期不会登录微信，也不会调用 Frida 或 WMPF。请改用微信网页会话或手动链接。",
        })),
        other => Err(EngineError::Message(format!("未知的目录来源：{other}"))),
    }
}

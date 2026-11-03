use crate::error::EngineError;
use crate::fetch::decode_text_bytes;
use serde::Serialize;
use std::path::PathBuf;

/// A way to turn an account identifier into a list of article URLs.
///
/// v1 only ships [`ManualProvider`]. Browser login and WMPF are named here so
/// the UI can show them as unavailable. They do not log in or attach to WeChat.
pub trait AccountProvider: Send + Sync {
    fn id(&self) -> &str;
    fn display_name(&self) -> &str;
    fn list_links(&self, request: &AccountRequest) -> Result<ProviderListing, EngineError>;
}

#[derive(Debug, Clone)]
pub struct AccountRequest {
    pub account_id: String,
    pub account_name: String,
    pub links_text: String,
    pub links_file: Option<PathBuf>,
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

impl AccountProvider for ManualProvider {
    fn id(&self) -> &str {
        "manual"
    }

    fn display_name(&self) -> &str {
        "手动链接文件"
    }

    fn list_links(&self, request: &AccountRequest) -> Result<ProviderListing, EngineError> {
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

struct UnavailableProvider {
    id: &'static str,
    name: &'static str,
    reason: &'static str,
}

impl AccountProvider for UnavailableProvider {
    fn id(&self) -> &str {
        self.id
    }

    fn display_name(&self) -> &str {
        self.name
    }

    fn list_links(&self, _request: &AccountRequest) -> Result<ProviderListing, EngineError> {
        Err(EngineError::Message(self.reason.into()))
    }
}

pub fn describe_providers() -> Vec<ProviderInfo> {
    vec![
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
        "wmpf" | "browser" | "wechat-login" => Ok(Box::new(UnavailableProvider {
            id: "wmpf",
            name: "微信登录 / WMPF",
            reason: "尚未实现。本期不会登录微信，也不会调用 Frida 或 WMPF。请改用手动链接文件。",
        })),
        other => Err(EngineError::Message(format!("未知的目录来源：{other}"))),
    }
}

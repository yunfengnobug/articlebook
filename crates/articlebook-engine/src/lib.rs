//! Local article archive engine used by the ArticleBook desktop app.
//!
//! The crate fetches pages you already have links for, extracts readable text,
//! and writes Markdown plus EPUB. A WeChat web provider can page a history list
//! only after the user saves a session from a login they completed themselves.
//! It does not start that login, impersonate the WeChat client, or call WMPF.

#![forbid(unsafe_code)]

mod book;
mod cache;
mod convert;
mod epub;
mod error;
mod extract;
mod fetch;
mod history;
mod images;
mod logging;
mod model;
mod paths;
mod pipeline;
mod provider;
mod rate;
mod urlutil;

pub use book::{draft_from_articles, render_book_markdown, write_book, BookDraft};
pub use convert::{render_markdown, render_xhtml};
pub use epub::{epub_mimetype, write_epub};
pub use error::EngineError;
pub use extract::{extract_html, source_hint_from_html};
pub use fetch::{
    decode_document, decode_text_bytes, Fetched, HttpClient, ReqwestClient, USER_AGENT,
};
pub use history::{
    collect_history, complete_session, describe_wechat_page, history_limit_or_default,
    load_session, parse_history_document, prepare_session, save_session, session_file,
    session_from_page_url, HistoryClient, HistoryEntry, HistoryKind, PageHint, SessionSummary,
    WechatWebSession, SAFETY_MAX_ARTICLES, SAFETY_MAX_PAGES, SESSION_FILE_NAME,
};
pub use logging::init_logging;
pub use model::{Article, ArticleKind, Block, FailureRecord, Inline};
pub use paths::{cli_app_dirs, suggest_output_dir, today_stamp, AppDirs};
pub use pipeline::{export_urls, ExportOptions, JobReport, NullSink, ProgressEvent, ProgressSink};
pub use provider::{
    describe_providers, provider_by_id, AccountProvider, AccountRequest, ManualProvider,
    ProviderInfo, ProviderListing, WechatWebProvider,
};
pub use urlutil::{normalize_url, parse_url_list, NormalizeOptions, ParsedUrls};

#[cfg(test)]
mod tests;

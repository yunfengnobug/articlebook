use serde::{Deserialize, Serialize};

/// Where an article's HTML came from. Drives extractor choice and labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArticleKind {
    Wechat,
    Generic,
}

impl ArticleKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Wechat => "微信文章",
            Self::Generic => "网页",
        }
    }
}

/// Inline content kept so Markdown and EPUB can be rendered from one tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Inline {
    Text { text: String },
    Strong { children: Vec<Inline> },
    Em { children: Vec<Inline> },
    Code { text: String },
    Link { href: String, children: Vec<Inline> },
    Image { src: String, alt: String },
    Break,
}

/// Block content of a cleaned article.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Block {
    Heading {
        level: u8,
        inlines: Vec<Inline>,
    },
    Paragraph {
        inlines: Vec<Inline>,
    },
    Quote {
        blocks: Vec<Block>,
    },
    List {
        ordered: bool,
        items: Vec<Vec<Block>>,
    },
    Code {
        lang: Option<String>,
        text: String,
    },
    Image {
        src: String,
        alt: String,
    },
    Table {
        rows: Vec<Vec<Vec<Inline>>>,
    },
    Rule,
    Placeholder {
        label: String,
    },
}

/// A URL that was skipped after retries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailureRecord {
    pub url: String,
    pub error: String,
    pub attempts: u32,
}

/// One extracted article, before it is written into a book.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Article {
    /// URL from the user's list, after normalization. This is the cache key.
    pub source_url: String,
    /// URL after redirects, used to resolve relative links.
    pub final_url: String,
    pub title: String,
    pub author: Option<String>,
    pub published_at: Option<String>,
    pub kind: ArticleKind,
    pub blocks: Vec<Block>,
}

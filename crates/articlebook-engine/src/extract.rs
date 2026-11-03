use crate::convert::{blocks_from_element, clean_text, inline_plain, plain_len};
use crate::error::EngineError;
use crate::model::{Article, ArticleKind, Block};
use crate::urlutil::url_is_wechat;
use scraper::{ElementRef, Html, Selector};

/// Read an optional source URL embedded by the offline fixtures and CLI.
pub fn source_hint_from_html(html: &str) -> Option<String> {
    let marker = "articlebook-source:";
    let lower = html.to_ascii_lowercase();
    let idx = lower.find(marker)?;
    let rest = html[idx + marker.len()..].trim_start();
    let end = rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
    let url = rest[..end].trim().trim_end_matches('-').trim();
    if url.starts_with("http://") || url.starts_with("https://") {
        Some(url.to_string())
    } else {
        None
    }
}

/// Choose the WeChat rules or the generic readability pass.
pub fn extract_html(html: &str, source_url: &str) -> Result<Article, EngineError> {
    let document = Html::parse_document(html);
    if is_wechat_document(&document, source_url) {
        extract_wechat(&document, source_url)
    } else {
        extract_generic(&document, source_url)
    }
}

fn is_wechat_document(document: &Html, url: &str) -> bool {
    if url_is_wechat(url) {
        return true;
    }
    select_first(document, "#js_content").is_some()
        && (select_first(document, "#activity-name").is_some()
            || document.root_element().html().contains("mp.weixin.qq.com"))
}

fn extract_wechat(document: &Html, url: &str) -> Result<Article, EngineError> {
    let content = select_first(document, "#js_content")
        .or_else(|| select_first(document, ".rich_media_content"))
        .ok_or_else(|| EngineError::Extract(wechat_missing_reason(document)))?;
    let title = text_of(document, "#activity-name")
        .or_else(|| meta_content(document, "meta[property='og:title']"))
        .or_else(|| title_tag(document))
        .unwrap_or_else(|| "未命名文章".into());
    let author = text_of(document, "#js_name")
        .or_else(|| text_of(document, ".rich_media_meta_nickname"))
        .or_else(|| meta_content(document, "meta[name='author']"));
    let published =
        text_of(document, "#publish_time").or_else(|| text_of(document, "em#publish_time"));
    let mut blocks = blocks_from_element(content, url);
    strip_duplicate_title(&title, &mut blocks);
    if plain_len(&blocks) < 12 {
        return Err(EngineError::Extract(
            "微信正文太短，或页面不是可阅读的文章".into(),
        ));
    }
    Ok(Article {
        source_url: url.to_string(),
        final_url: url.to_string(),
        title,
        author,
        published_at: published,
        kind: ArticleKind::Wechat,
        blocks,
    })
}

fn wechat_missing_reason(document: &Html) -> String {
    let text = document.root_element().text().collect::<String>();
    if ["环境异常", "去验证", "操作频繁"]
        .iter()
        .any(|item| text.contains(item))
    {
        return "这是微信的验证页，不是文章正文。请在浏览器里打开你有权阅读的链接后再归档；本工具不会尝试绕过验证。".into();
    }
    if text.contains("该内容已被发布者删除") || text.contains("此内容因违规无法查看")
    {
        return "文章已删除或无法查看".into();
    }
    "没有找到微信正文".into()
}

fn extract_generic(document: &Html, url: &str) -> Result<Article, EngineError> {
    let root = pick_root(document)?;
    let mut blocks = blocks_from_element(root, url);
    let title = meta_content(document, "meta[property='og:title']")
        .or_else(|| first_heading(&blocks))
        .or_else(|| title_tag(document))
        .unwrap_or_else(|| "未命名文章".into());
    strip_duplicate_title(&title, &mut blocks);
    if plain_len(&blocks) < 20 {
        return Err(EngineError::Extract("没有提取到足够的正文".into()));
    }
    let author = meta_content(document, "meta[name='author']")
        .or_else(|| meta_content(document, "meta[property='article:author']"));
    let published = meta_content(document, "meta[property='article:published_time']")
        .or_else(|| meta_content(document, "meta[name='pubdate']"))
        .or_else(|| time_datetime(document));
    Ok(Article {
        source_url: url.to_string(),
        final_url: url.to_string(),
        title,
        author,
        published_at: published,
        kind: ArticleKind::Generic,
        blocks,
    })
}

fn pick_root(document: &Html) -> Result<ElementRef<'_>, EngineError> {
    for selector in [
        "article",
        "main",
        "[role='main']",
        "#content",
        ".post-content",
        ".entry-content",
        ".article-content",
        ".markdown-body",
    ] {
        if let Some(el) = select_first(document, selector) {
            if text_len(el) >= 40 {
                return Ok(tighten(el));
            }
        }
    }
    let selector = Selector::parse("div, section, td, article").expect("selector");
    let mut best: Option<(i32, ElementRef<'_>)> = None;
    for el in document.select(&selector) {
        let score = score_element(el);
        if best.as_ref().map(|(prev, _)| score > *prev).unwrap_or(true) {
            best = Some((score, el));
        }
    }
    match best {
        Some((score, el)) if score >= 30 => Ok(tighten(el)),
        _ => Err(EngineError::Extract("没有找到正文容器".into())),
    }
}

fn score_element(el: ElementRef<'_>) -> i32 {
    let len = text_len(el) as i32;
    if len < 40 {
        return 0;
    }
    let paragraphs = Selector::parse("p").expect("p");
    let anchors = Selector::parse("a").expect("a");
    let p_count = el.select(&paragraphs).count() as i32;
    let mut score = len / 8 + p_count * 25;
    let link_len: usize = el.select(&anchors).map(text_len).sum();
    if link_len.saturating_mul(100) / (len as usize).max(1) > 60 {
        score /= 3;
    }
    score
}

fn tighten(el: ElementRef<'_>) -> ElementRef<'_> {
    let mut current = el;
    for _ in 0..8 {
        let mine = text_len(current);
        if mine == 0 {
            break;
        }
        let mut best: Option<(usize, ElementRef<'_>)> = None;
        for child in current.children().filter_map(ElementRef::wrap) {
            let name = child.value().name();
            if !matches!(name, "div" | "section" | "article" | "main" | "td") {
                continue;
            }
            let len = text_len(child);
            if best.as_ref().map(|(prev, _)| len > *prev).unwrap_or(true) {
                best = Some((len, child));
            }
        }
        if let Some((len, child)) = best {
            if len.saturating_mul(100) / mine >= 90 {
                current = child;
                continue;
            }
        }
        break;
    }
    current
}

fn text_len(el: ElementRef<'_>) -> usize {
    el.text()
        .flat_map(|text| text.chars())
        .filter(|c| !c.is_whitespace())
        .count()
}

fn select_first<'a>(document: &'a Html, selector: &str) -> Option<ElementRef<'a>> {
    let parsed = Selector::parse(selector).ok()?;
    document.select(&parsed).next()
}

fn text_of(document: &Html, selector: &str) -> Option<String> {
    let el = select_first(document, selector)?;
    let text = clean_text(&el.text().collect::<String>());
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn meta_content(document: &Html, selector: &str) -> Option<String> {
    let el = select_first(document, selector)?;
    let text = clean_text(el.value().attr("content")?);
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn title_tag(document: &Html) -> Option<String> {
    let text = text_of(document, "title")?;
    let text = text
        .trim_end_matches("- 微信公众平台")
        .trim_end_matches(" - 微信公众平台")
        .trim()
        .to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn time_datetime(document: &Html) -> Option<String> {
    let el = select_first(document, "time[datetime]")?;
    let value = el.value().attr("datetime")?.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn first_heading(blocks: &[Block]) -> Option<String> {
    let Block::Heading { inlines, .. } = blocks.first()? else {
        return None;
    };
    let text = clean_text(&inline_plain(inlines));
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn strip_duplicate_title(title: &str, blocks: &mut Vec<Block>) {
    let Some(Block::Heading { inlines, .. }) = blocks.first() else {
        return;
    };
    if clean_text(&inline_plain(inlines)) == clean_text(title) {
        blocks.remove(0);
    }
}

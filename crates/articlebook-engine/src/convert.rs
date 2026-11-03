use crate::model::{Block, Inline};
use scraper::{ElementRef, Node};
use std::ops::Deref;

pub(crate) fn blocks_from_element(el: ElementRef<'_>, base: &str) -> Vec<Block> {
    blocks_from_element_inner(el, base, true)
}

fn blocks_from_element_inner(el: ElementRef<'_>, base: &str, as_root: bool) -> Vec<Block> {
    if !as_root {
        if let Some(label) = placeholder_label(el) {
            return vec![Block::Placeholder { label }];
        }
        if should_skip(el) {
            return Vec::new();
        }
    }
    let name = el.value().name().to_ascii_lowercase();
    match name.as_str() {
        "script" | "style" | "noscript" | "svg" | "iframe" | "form" | "button" | "input"
        | "textarea" | "select" | "canvas" | "nav" | "footer" | "aside" => {
            if as_root {
                children_blocks(el, base)
            } else {
                Vec::new()
            }
        }
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let level = name[1..].parse::<u8>().unwrap_or(1).clamp(1, 6);
            let inlines = normalize_inlines(collect_inlines(el, base));
            if inlines.is_empty() {
                Vec::new()
            } else {
                vec![Block::Heading { level, inlines }]
            }
        }
        "p" | "figcaption" => paragraph(el, base),
        "blockquote" => {
            let inner = children_blocks(el, base);
            if inner.is_empty() {
                Vec::new()
            } else {
                vec![Block::Quote { blocks: inner }]
            }
        }
        "ul" => vec![Block::List {
            ordered: false,
            items: list_items(el, base),
        }],
        "ol" => vec![Block::List {
            ordered: true,
            items: list_items(el, base),
        }],
        "pre" => code_block(el),
        "hr" => vec![Block::Rule],
        "table" => table_block(el, base),
        "img" => img_inline(el, base)
            .map(|inline| match inline {
                Inline::Image { src, alt } => Block::Image { src, alt },
                _ => Block::Rule,
            })
            .into_iter()
            .collect(),
        "li" => children_blocks(el, base),
        _ => children_blocks(el, base),
    }
}

fn children_blocks(el: ElementRef<'_>, base: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut pending = Vec::new();
    for child in el.children() {
        match child.value() {
            Node::Text(text) => push_text(&mut pending, text.deref()),
            Node::Element(_) => {
                let Some(er) = ElementRef::wrap(child) else {
                    continue;
                };
                if placeholder_label(er).is_some() || !is_inline_tag(er.value().name()) {
                    flush_pending(&mut pending, &mut blocks);
                    blocks.extend(blocks_from_element_inner(er, base, false));
                } else if should_skip(er) {
                    continue;
                } else {
                    pending.extend(inlines_from(er, base));
                }
            }
            _ => {}
        }
    }
    flush_pending(&mut pending, &mut blocks);
    blocks
}

fn paragraph(el: ElementRef<'_>, base: &str) -> Vec<Block> {
    let inlines = normalize_inlines(collect_inlines(el, base));
    if inlines.is_empty() {
        Vec::new()
    } else if inlines
        .iter()
        .all(|inline| matches!(inline, Inline::Image { .. } | Inline::Break))
    {
        inlines
            .into_iter()
            .filter_map(|inline| match inline {
                Inline::Image { src, alt } => Some(Block::Image { src, alt }),
                _ => None,
            })
            .collect()
    } else {
        vec![Block::Paragraph { inlines }]
    }
}

fn list_items(el: ElementRef<'_>, base: &str) -> Vec<Vec<Block>> {
    let mut items = Vec::new();
    for child in el.children() {
        let Some(er) = ElementRef::wrap(child) else {
            continue;
        };
        if er.value().name() != "li" {
            continue;
        }
        let mut blocks = children_blocks(er, base);
        if blocks.is_empty() {
            blocks.push(Block::Paragraph {
                inlines: vec![Inline::Text {
                    text: String::new(),
                }],
            });
        }
        items.push(blocks);
    }
    items
}

fn code_block(el: ElementRef<'_>) -> Vec<Block> {
    let mut text = el.text().collect::<String>();
    if text.starts_with('\n') {
        text.remove(0);
    }
    while text.ends_with('\n') {
        text.pop();
    }
    if text.trim().is_empty() {
        return Vec::new();
    }
    let lang = el.children().find_map(|child| {
        let er = ElementRef::wrap(child)?;
        if er.value().name() == "code" {
            code_lang(er)
        } else {
            None
        }
    });
    vec![Block::Code { lang, text }]
}

fn code_lang(el: ElementRef<'_>) -> Option<String> {
    let class = el.value().attr("class").unwrap_or("");
    for token in class.split_whitespace() {
        if let Some(rest) = token
            .strip_prefix("language-")
            .or_else(|| token.strip_prefix("lang-"))
        {
            if !rest.is_empty() {
                return Some(rest.to_string());
            }
        }
    }
    None
}

fn table_block(el: ElementRef<'_>, base: &str) -> Vec<Block> {
    let mut rows = Vec::new();
    for child in descendants_named(el, "tr") {
        let cells: Vec<Vec<Inline>> = element_children(child)
            .filter(|cell| matches!(cell.value().name(), "td" | "th"))
            .map(|cell| normalize_inlines(collect_inlines(cell, base)))
            .filter(|inlines| !inlines.is_empty())
            .collect();
        if !cells.is_empty() {
            rows.push(cells);
        }
    }
    if rows.is_empty() {
        children_blocks(el, base)
    } else {
        vec![Block::Table { rows }]
    }
}

fn descendants_named<'a>(el: ElementRef<'a>, name: &str) -> Vec<ElementRef<'a>> {
    let mut out = Vec::new();
    fn walk<'a>(el: ElementRef<'a>, name: &str, out: &mut Vec<ElementRef<'a>>) {
        for child in element_children(el) {
            if child.value().name() == name {
                out.push(child);
            }
            walk(child, name, out);
        }
    }
    walk(el, name, &mut out);
    out
}

fn collect_inlines(el: ElementRef<'_>, base: &str) -> Vec<Inline> {
    let mut out = Vec::new();
    for child in el.children() {
        match child.value() {
            Node::Text(text) => push_text(&mut out, text.deref()),
            Node::Element(_) => {
                let Some(er) = ElementRef::wrap(child) else {
                    continue;
                };
                if should_skip(er) && placeholder_label(er).is_none() {
                    continue;
                }
                if is_inline_tag(er.value().name()) {
                    out.extend(inlines_from(er, base));
                } else if let Some(label) = placeholder_label(er) {
                    out.push(Inline::Text { text: label });
                } else {
                    let text = er.text().collect::<String>();
                    push_text(&mut out, &text);
                    out.extend(images_in(er, base));
                }
            }
            _ => {}
        }
    }
    out
}

fn inlines_from(el: ElementRef<'_>, base: &str) -> Vec<Inline> {
    let name = el.value().name().to_ascii_lowercase();
    match name.as_str() {
        "br" => vec![Inline::Break],
        "img" => img_inline(el, base).into_iter().collect(),
        "strong" | "b" => {
            let children = normalize_inlines(collect_inlines(el, base));
            if children.is_empty() {
                Vec::new()
            } else {
                vec![Inline::Strong { children }]
            }
        }
        "em" | "i" => {
            let children = normalize_inlines(collect_inlines(el, base));
            if children.is_empty() {
                Vec::new()
            } else {
                vec![Inline::Em { children }]
            }
        }
        "code" | "kbd" | "samp" => {
            let text = el.text().collect::<String>();
            let text = text.trim().to_string();
            if text.is_empty() {
                Vec::new()
            } else {
                vec![Inline::Code { text }]
            }
        }
        "a" => {
            let children = normalize_inlines(collect_inlines(el, base));
            if children.is_empty() {
                Vec::new()
            } else if let Some(href) = el
                .value()
                .attr("href")
                .and_then(|href| safe_href(href, base))
            {
                vec![Inline::Link { href, children }]
            } else {
                children
            }
        }
        _ => collect_inlines(el, base),
    }
}

fn images_in(el: ElementRef<'_>, base: &str) -> Vec<Inline> {
    let mut out = Vec::new();
    if el.value().name() == "img" {
        if let Some(image) = img_inline(el, base) {
            out.push(image);
        }
    }
    for child in element_children(el) {
        out.extend(images_in(child, base));
    }
    out
}

fn img_inline(el: ElementRef<'_>, base: &str) -> Option<Inline> {
    let raw = ["data-src", "data-original", "data-actualsrc", "src"]
        .into_iter()
        .find_map(|key| {
            let value = el.value().attr(key)?.trim();
            if value.is_empty() {
                return None;
            }
            if key == "src" && (value.starts_with("data:") || value.contains("blank.gif")) {
                return None;
            }
            Some(value)
        })?;
    let src = resolve_url(base, raw);
    let alt = el.value().attr("alt").map(clean_text).unwrap_or_default();
    Some(Inline::Image { src, alt })
}

fn safe_href(raw: &str, base: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.starts_with('#') {
        return None;
    }
    let lower = raw.to_ascii_lowercase();
    if lower.starts_with("javascript:") || lower.starts_with("data:") {
        return None;
    }
    Some(resolve_url(base, raw))
}

pub(crate) fn resolve_url(base: &str, raw: &str) -> String {
    let raw = raw.trim();
    if let Some(rest) = raw.strip_prefix("//") {
        return format!("https://{rest}");
    }
    if raw.starts_with("http://") || raw.starts_with("https://") || raw.starts_with("data:") {
        return raw.to_string();
    }
    if let Ok(base) = url::Url::parse(base) {
        if let Ok(joined) = base.join(raw) {
            return joined.to_string();
        }
    }
    raw.to_string()
}

fn push_text(out: &mut Vec<Inline>, raw: &str) {
    if raw.chars().all(|c| c.is_whitespace()) {
        if out.is_empty() {
            return;
        }
        if let Some(Inline::Text { text }) = out.last_mut() {
            if !text.ends_with(' ') {
                text.push(' ');
            }
        } else {
            out.push(Inline::Text { text: " ".into() });
        }
        return;
    }
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return;
    }
    let mut text = String::new();
    if raw.chars().next().is_some_and(|c| c.is_whitespace()) {
        text.push(' ');
    }
    text.push_str(&collapsed);
    if raw.chars().rev().next().is_some_and(|c| c.is_whitespace()) {
        text.push(' ');
    }
    out.push(Inline::Text { text });
}

fn normalize_inlines(inlines: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::new();
    for inline in inlines {
        match inline {
            Inline::Text { text } => {
                if text.is_empty() {
                    continue;
                }
                if let Some(Inline::Text { text: prev }) = out.last_mut() {
                    prev.push_str(&text);
                } else {
                    out.push(Inline::Text { text });
                }
            }
            Inline::Strong { children } => {
                let children = normalize_inlines(children);
                if !children.is_empty() {
                    out.push(Inline::Strong { children });
                }
            }
            Inline::Em { children } => {
                let children = normalize_inlines(children);
                if !children.is_empty() {
                    out.push(Inline::Em { children });
                }
            }
            Inline::Link { href, children } => {
                let children = normalize_inlines(children);
                if !children.is_empty() {
                    out.push(Inline::Link { href, children });
                }
            }
            other => out.push(other),
        }
    }
    if let Some(Inline::Text { text }) = out.first_mut() {
        let trimmed = text.trim_start().to_string();
        if trimmed.is_empty() {
            out.remove(0);
        } else {
            *text = trimmed;
        }
    }
    if let Some(Inline::Text { text }) = out.last_mut() {
        let trimmed = text.trim_end().to_string();
        if trimmed.is_empty() {
            out.pop();
        } else {
            *text = trimmed;
        }
    }
    for inline in &mut out {
        if let Inline::Text { text } = inline {
            *text = tighten_spaces(text);
        }
    }
    out.retain(|inline| !matches!(inline, Inline::Text { text } if text.is_empty()));
    out
}

fn tighten_spaces(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut prev_space = false;
    for (index, ch) in chars.iter().copied().enumerate() {
        if ch == ' ' || ch == '\u{00a0}' {
            let prev = chars[..index]
                .iter()
                .rev()
                .find(|c| **c != ' ' && **c != '\u{00a0}');
            let next = chars[index + 1..]
                .iter()
                .find(|c| **c != ' ' && **c != '\u{00a0}');
            if let (Some(prev), Some(next)) = (prev, next) {
                if is_cjk(*prev) && is_cjk(*next) {
                    prev_space = false;
                    continue;
                }
            }
            if !prev_space {
                out.push(' ');
            }
            prev_space = true;
        } else {
            prev_space = false;
            out.push(ch);
        }
    }
    out
}

fn is_cjk(ch: char) -> bool {
    matches!(
        ch,
        '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '\u{3000}'..='\u{303F}' | '\u{FF00}'..='\u{FFEF}'
    )
}

fn flush_pending(pending: &mut Vec<Inline>, blocks: &mut Vec<Block>) {
    let inlines = normalize_inlines(std::mem::take(pending));
    if inlines.is_empty() {
        return;
    }
    if inlines
        .iter()
        .all(|inline| matches!(inline, Inline::Image { .. } | Inline::Break))
    {
        for inline in inlines {
            if let Inline::Image { src, alt } = inline {
                blocks.push(Block::Image { src, alt });
            }
        }
    } else {
        blocks.push(Block::Paragraph { inlines });
    }
}

fn is_inline_tag(name: &str) -> bool {
    matches!(
        name,
        "a" | "span"
            | "strong"
            | "b"
            | "em"
            | "i"
            | "code"
            | "sub"
            | "sup"
            | "u"
            | "s"
            | "small"
            | "mark"
            | "font"
            | "br"
            | "img"
            | "wbr"
            | "label"
            | "time"
            | "abbr"
            | "cite"
            | "q"
            | "kbd"
            | "samp"
            | "var"
            | "bdo"
    )
}

fn should_skip(el: ElementRef<'_>) -> bool {
    let name = el.value().name();
    if matches!(
        name,
        "nav" | "footer" | "aside" | "form" | "script" | "style" | "noscript" | "iframe"
    ) {
        return true;
    }
    if let Some(role) = el.value().attr("role") {
        let role = role.to_ascii_lowercase();
        if matches!(
            role.as_str(),
            "navigation" | "banner" | "contentinfo" | "complementary"
        ) {
            return true;
        }
    }
    let mut tokens = Vec::new();
    if let Some(id) = el.value().id() {
        push_tokens(&mut tokens, id);
    }
    if let Some(class) = el.value().attr("class") {
        for part in class.split_whitespace() {
            push_tokens(&mut tokens, part);
        }
    }
    if tokens.iter().any(|token| is_noise_token(token)) {
        return true;
    }
    let raw = format!(
        "{} {}",
        el.value().id().unwrap_or(""),
        el.value().attr("class").unwrap_or("")
    )
    .to_ascii_lowercase();
    raw.contains("qr_code") || raw.contains("qrcode")
}

fn push_tokens(out: &mut Vec<String>, raw: &str) {
    let lower = raw.to_ascii_lowercase();
    out.push(lower.clone());
    for part in lower.split(|c: char| !c.is_ascii_alphanumeric()) {
        if !part.is_empty() {
            out.push(part.to_string());
        }
    }
}

fn is_noise_token(token: &str) -> bool {
    matches!(
        token,
        "nav"
            | "menu"
            | "footer"
            | "header"
            | "sidebar"
            | "comment"
            | "comments"
            | "related"
            | "breadcrumb"
            | "pager"
            | "pagination"
            | "advert"
            | "ads"
            | "advertisement"
            | "cookie"
            | "subscribe"
            | "promo"
            | "recommend"
            | "social"
            | "navbar"
            | "navigation"
            | "share"
            | "sharing"
            | "aside"
            | "topbar"
            | "qrcode"
    ) || token.ends_with("-nav")
        || token.starts_with("nav-")
        || token.contains("sidebar")
        || token.contains("comment")
        || token.contains("breadcrumb")
        || token.contains("advert")
        || token.contains("recommend")
}

fn placeholder_label(el: ElementRef<'_>) -> Option<String> {
    let name = el.value().name().to_ascii_lowercase();
    if name.starts_with("mp-")
        || matches!(
            name.as_str(),
            "mpvoice" | "qqmusic" | "mplive" | "mpvideosnap"
        )
    {
        return Some(label_for_tag(&name));
    }
    let raw = format!(
        "{} {}",
        el.value().id().unwrap_or(""),
        el.value().attr("class").unwrap_or("")
    )
    .to_ascii_lowercase();
    if raw.contains("vote_area") || raw.contains("js_vote") {
        return Some("互动组件：投票".into());
    }
    None
}

fn label_for_tag(tag: &str) -> String {
    let kind = if tag.contains("vote") {
        "投票"
    } else if tag.contains("video") || tag == "mpvoice" || tag.contains("videosnap") {
        "音视频"
    } else if tag.contains("product") {
        "商品卡"
    } else if tag.contains("miniprogram") || tag.contains("weapp") {
        "小程序"
    } else if tag.contains("profile") {
        "账号名片"
    } else if tag.contains("music") || tag == "qqmusic" {
        "音乐"
    } else if tag.contains("live") {
        "直播"
    } else {
        "互动"
    };
    format!("互动组件：{kind}")
}

fn element_children(el: ElementRef<'_>) -> impl Iterator<Item = ElementRef<'_>> {
    el.children().filter_map(ElementRef::wrap)
}

pub fn render_markdown(blocks: &[Block]) -> String {
    let mut out = String::new();
    render_blocks(blocks, &mut out);
    let trimmed = out.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("{trimmed}\n")
    }
}

fn render_blocks(blocks: &[Block], out: &mut String) {
    for block in blocks {
        match block {
            Block::Heading { level, inlines } => {
                let level = (*level).clamp(1, 6) as usize;
                out.push_str(&"#".repeat(level));
                out.push(' ');
                out.push_str(&render_inlines_md(inlines).replace('\n', " "));
                out.push_str("\n\n");
            }
            Block::Paragraph { inlines } => {
                out.push_str(&render_inlines_md(inlines));
                out.push_str("\n\n");
            }
            Block::Quote { blocks } => {
                let mut inner = String::new();
                render_blocks(blocks, &mut inner);
                for line in inner.trim_end().lines() {
                    out.push_str("> ");
                    out.push_str(line);
                    out.push('\n');
                }
                out.push('\n');
            }
            Block::List { ordered, items } => {
                for (index, item) in items.iter().enumerate() {
                    let marker = if *ordered {
                        format!("{}. ", index + 1)
                    } else {
                        "- ".to_string()
                    };
                    let mut inner = String::new();
                    render_blocks(item, &mut inner);
                    let pad = " ".repeat(marker.chars().count());
                    let lines: Vec<&str> = inner.trim_end().lines().collect();
                    if lines.is_empty() {
                        out.push_str(&marker);
                        out.push('\n');
                        continue;
                    }
                    for (line_index, line) in lines.iter().enumerate() {
                        if line_index == 0 {
                            out.push_str(&marker);
                        } else {
                            out.push_str(&pad);
                        }
                        out.push_str(line);
                        out.push('\n');
                    }
                }
                out.push('\n');
            }
            Block::Code { lang, text } => {
                out.push_str("```");
                if let Some(lang) = lang {
                    out.push_str(lang);
                }
                out.push('\n');
                out.push_str(text);
                out.push_str("\n```\n\n");
            }
            Block::Image { src, alt } => {
                out.push_str("![");
                out.push_str(&escape_md(alt));
                out.push_str("](");
                out.push_str(&escape_md_url(src));
                out.push_str(")\n\n");
            }
            Block::Table { rows } => {
                if rows.is_empty() {
                    continue;
                }
                let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
                let render_row = |row: &Vec<Vec<Inline>>, out: &mut String| {
                    out.push('|');
                    for index in 0..cols {
                        out.push(' ');
                        if let Some(cell) = row.get(index) {
                            out.push_str(
                                &render_inlines_md(cell)
                                    .replace('|', "\\|")
                                    .replace('\n', " "),
                            );
                        }
                        out.push_str(" |");
                    }
                    out.push('\n');
                };
                render_row(&rows[0], out);
                out.push('|');
                for _ in 0..cols {
                    out.push_str(" --- |");
                }
                out.push('\n');
                for row in rows.iter().skip(1) {
                    render_row(row, out);
                }
                out.push('\n');
            }
            Block::Rule => out.push_str("---\n\n"),
            Block::Placeholder { label } => {
                out.push_str("> ");
                out.push_str(label);
                out.push_str("\n\n");
            }
        }
    }
}

fn render_inlines_md(inlines: &[Inline]) -> String {
    let mut out = String::new();
    for inline in inlines {
        match inline {
            Inline::Text { text } => out.push_str(&escape_md(text)),
            Inline::Strong { children } => {
                out.push_str("**");
                out.push_str(&render_inlines_md(children));
                out.push_str("**");
            }
            Inline::Em { children } => {
                out.push('*');
                out.push_str(&render_inlines_md(children));
                out.push('*');
            }
            Inline::Code { text } => {
                out.push('`');
                out.push_str(&text.replace('`', "'"));
                out.push('`');
            }
            Inline::Link { href, children } => {
                out.push('[');
                out.push_str(&render_inlines_md(children));
                out.push_str("](");
                out.push_str(&escape_md_url(href));
                out.push(')');
            }
            Inline::Image { src, alt } => {
                out.push_str("![");
                out.push_str(&escape_md(alt));
                out.push_str("](");
                out.push_str(&escape_md_url(src));
                out.push(')');
            }
            Inline::Break => out.push_str("  \n"),
        }
    }
    out
}

fn escape_md(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if matches!(ch, '\\' | '`' | '*' | '_' | '[' | ']') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

fn escape_md_url(url: &str) -> String {
    url.replace(' ', "%20")
        .replace(')', "%29")
        .replace('(', "%28")
}

pub fn render_xhtml(blocks: &[Block]) -> String {
    let mut out = String::new();
    render_xhtml_blocks(blocks, &mut out);
    out
}

fn render_xhtml_blocks(blocks: &[Block], out: &mut String) {
    for block in blocks {
        match block {
            Block::Heading { level, inlines } => {
                let level = (*level).clamp(1, 6);
                out.push_str(&format!("<h{level}>"));
                render_xhtml_inlines(inlines, out);
                out.push_str(&format!("</h{level}>\n"));
            }
            Block::Paragraph { inlines } => {
                out.push_str("<p>");
                render_xhtml_inlines(inlines, out);
                out.push_str("</p>\n");
            }
            Block::Quote { blocks } => {
                out.push_str("<blockquote>\n");
                render_xhtml_blocks(blocks, out);
                out.push_str("</blockquote>\n");
            }
            Block::List { ordered, items } => {
                let tag = if *ordered { "ol" } else { "ul" };
                out.push_str(&format!("<{tag}>\n"));
                for item in items {
                    out.push_str("<li>\n");
                    render_xhtml_blocks(item, out);
                    out.push_str("</li>\n");
                }
                out.push_str(&format!("</{tag}>\n"));
            }
            Block::Code { text, .. } => {
                out.push_str("<pre><code>");
                out.push_str(&xml_escape(text));
                out.push_str("</code></pre>\n");
            }
            Block::Image { src, alt } => {
                out.push_str("<p><img src=\"");
                out.push_str(&xml_escape(src));
                out.push_str("\" alt=\"");
                out.push_str(&xml_escape(alt));
                out.push_str("\"/></p>\n");
            }
            Block::Table { rows } => {
                out.push_str("<table>\n");
                for (index, row) in rows.iter().enumerate() {
                    out.push_str("<tr>");
                    let cell = if index == 0 { "th" } else { "td" };
                    for item in row {
                        out.push_str(&format!("<{cell}>"));
                        render_xhtml_inlines(item, out);
                        out.push_str(&format!("</{cell}>"));
                    }
                    out.push_str("</tr>\n");
                }
                out.push_str("</table>\n");
            }
            Block::Rule => out.push_str("<hr/>\n"),
            Block::Placeholder { label } => {
                out.push_str("<p class=\"placeholder\">");
                out.push_str(&xml_escape(label));
                out.push_str("</p>\n");
            }
        }
    }
}

fn render_xhtml_inlines(inlines: &[Inline], out: &mut String) {
    for inline in inlines {
        match inline {
            Inline::Text { text } => out.push_str(&xml_escape(text)),
            Inline::Strong { children } => {
                out.push_str("<strong>");
                render_xhtml_inlines(children, out);
                out.push_str("</strong>");
            }
            Inline::Em { children } => {
                out.push_str("<em>");
                render_xhtml_inlines(children, out);
                out.push_str("</em>");
            }
            Inline::Code { text } => {
                out.push_str("<code>");
                out.push_str(&xml_escape(text));
                out.push_str("</code>");
            }
            Inline::Link { href, children } => {
                out.push_str("<a href=\"");
                out.push_str(&xml_escape(href));
                out.push_str("\">");
                render_xhtml_inlines(children, out);
                out.push_str("</a>");
            }
            Inline::Image { src, alt } => {
                out.push_str("<img src=\"");
                out.push_str(&xml_escape(src));
                out.push_str("\" alt=\"");
                out.push_str(&xml_escape(alt));
                out.push_str("\"/>");
            }
            Inline::Break => out.push_str("<br/>"),
        }
    }
}

pub fn xml_escape(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

pub(crate) fn map_image_srcs(blocks: &mut [Block], map: &mut impl FnMut(&str) -> String) {
    for block in blocks {
        match block {
            Block::Heading { inlines, .. } | Block::Paragraph { inlines } => {
                map_inline_srcs(inlines, map);
            }
            Block::Quote { blocks } => map_image_srcs(blocks, map),
            Block::List { items, .. } => {
                for item in items {
                    map_image_srcs(item, map);
                }
            }
            Block::Image { src, .. } => *src = map(src),
            Block::Table { rows } => {
                for row in rows {
                    for cell in row {
                        map_inline_srcs(cell, map);
                    }
                }
            }
            Block::Code { .. } | Block::Rule | Block::Placeholder { .. } => {}
        }
    }
}

fn map_inline_srcs(inlines: &mut [Inline], map: &mut impl FnMut(&str) -> String) {
    for inline in inlines {
        match inline {
            Inline::Image { src, .. } => *src = map(src),
            Inline::Strong { children }
            | Inline::Em { children }
            | Inline::Link { children, .. } => {
                map_inline_srcs(children, map);
            }
            Inline::Text { .. } | Inline::Code { .. } | Inline::Break => {}
        }
    }
}

pub fn plain_text(blocks: &[Block]) -> String {
    let mut out = String::new();
    fn walk(blocks: &[Block], out: &mut String) {
        for block in blocks {
            match block {
                Block::Heading { inlines, .. } | Block::Paragraph { inlines } => {
                    out.push_str(&inline_plain(inlines));
                    out.push('\n');
                }
                Block::Quote { blocks } => walk(blocks, out),
                Block::List { items, .. } => {
                    for item in items {
                        walk(item, out);
                    }
                }
                Block::Code { text, .. } => {
                    out.push_str(text);
                    out.push('\n');
                }
                Block::Image { alt, .. } => {
                    out.push_str(alt);
                    out.push('\n');
                }
                Block::Table { rows } => {
                    for row in rows {
                        for cell in row {
                            out.push_str(&inline_plain(cell));
                            out.push(' ');
                        }
                    }
                }
                Block::Placeholder { label } => {
                    out.push_str(label);
                    out.push('\n');
                }
                Block::Rule => {}
            }
        }
    }
    walk(blocks, &mut out);
    out
}

pub fn inline_plain(inlines: &[Inline]) -> String {
    let mut out = String::new();
    for inline in inlines {
        match inline {
            Inline::Text { text } | Inline::Code { text } => out.push_str(text),
            Inline::Strong { children }
            | Inline::Em { children }
            | Inline::Link { children, .. } => {
                out.push_str(&inline_plain(children));
            }
            Inline::Image { alt, .. } => out.push_str(alt),
            Inline::Break => out.push(' '),
        }
    }
    out
}

pub fn clean_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn plain_len(blocks: &[Block]) -> usize {
    plain_text(blocks)
        .chars()
        .filter(|c| !c.is_whitespace())
        .count()
}

use crate::convert::{map_image_srcs, render_markdown};
use crate::error::EngineError;
use crate::model::{Article, FailureRecord};
use std::path::Path;

pub struct Chapter {
    pub number: usize,
    pub source_index: usize,
    pub anchor: String,
    pub article: Article,
}

pub struct BookDraft {
    pub title: String,
    pub author: String,
    pub chapters: Vec<Chapter>,
}

pub fn draft_from_articles(
    title: String,
    author: String,
    articles: Vec<(usize, Article)>,
) -> BookDraft {
    let chapters = articles
        .into_iter()
        .enumerate()
        .map(|(index, (source_index, article))| {
            let number = index + 1;
            Chapter {
                anchor: format!("article-{number}"),
                number,
                source_index,
                article,
            }
        })
        .collect();
    BookDraft {
        title,
        author,
        chapters,
    }
}

pub fn render_book_markdown(book: &BookDraft) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", heading_text(&book.title)));
    if !book.author.trim().is_empty() {
        out.push_str(&format!("署名：{}\n\n", book.author.trim()));
    }
    out.push_str("本文集由 ArticleBook 在本地整理，供个人阅读。请只归档你有权保存的文章。\n\n");
    out.push_str("## 目录\n\n");
    if book.chapters.is_empty() {
        out.push_str("（没有成功归档的文章）\n\n");
    } else {
        for chapter in &book.chapters {
            out.push_str(&format!(
                "{}. [{}](#{})\n",
                chapter.number,
                link_text(&chapter.article.title),
                chapter.anchor
            ));
        }
        out.push('\n');
    }
    for chapter in &book.chapters {
        out.push_str("---\n\n");
        out.push_str(&format!("<a id=\"{}\"></a>\n\n", chapter.anchor));
        out.push_str(&chapter_markdown(chapter, false));
    }
    out
}

pub fn render_article_file(chapter: &Chapter) -> String {
    chapter_markdown(chapter, true)
}

fn chapter_markdown(chapter: &Chapter, rebase_images: bool) -> String {
    let mut article = chapter.article.clone();
    if rebase_images {
        map_image_srcs(&mut article.blocks, &mut |src| {
            if src.starts_with("images/") {
                format!("../{src}")
            } else {
                src.to_string()
            }
        });
    }
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", heading_text(&article.title)));
    out.push_str(&format!("- 来源：{}\n", article.source_url));
    if article.final_url != article.source_url && !article.final_url.is_empty() {
        out.push_str(&format!("- 最终地址：{}\n", article.final_url));
    }
    if let Some(author) = article.author.as_deref().filter(|s| !s.is_empty()) {
        out.push_str(&format!("- 作者：{author}\n"));
    }
    if let Some(date) = article.published_at.as_deref().filter(|s| !s.is_empty()) {
        out.push_str(&format!("- 时间：{date}\n"));
    }
    out.push_str(&format!("- 类型：{}\n", article.kind.label()));
    out.push_str(&format!("- 列表序号：{}\n\n", chapter.source_index));
    out.push_str(&render_markdown(&article.blocks));
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

pub fn write_book(book: &BookDraft, output_dir: &Path) -> Result<(), EngineError> {
    std::fs::create_dir_all(output_dir.join("articles"))?;
    std::fs::write(output_dir.join("book.md"), render_book_markdown(book))?;
    for chapter in &book.chapters {
        let path = output_dir
            .join("articles")
            .join(format!("{:03}.md", chapter.number));
        std::fs::write(path, render_article_file(chapter))?;
    }
    Ok(())
}

pub fn render_failures(failures: &[FailureRecord]) -> String {
    let mut out = String::from("# 失败清单\n\n");
    if failures.is_empty() {
        out.push_str("本次没有失败的链接。\n");
        return out;
    }
    for failure in failures {
        out.push_str(&format!(
            "- {} — {}（尝试 {} 次）\n",
            failure.url, failure.error, failure.attempts
        ));
    }
    out
}

fn heading_text(title: &str) -> String {
    title
        .replace(['\n', '\r'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn link_text(title: &str) -> String {
    heading_text(title).replace(['[', ']'], "")
}

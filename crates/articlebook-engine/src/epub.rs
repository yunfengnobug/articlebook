use crate::book::BookDraft;
use crate::convert::{render_xhtml, xml_escape};
use crate::error::EngineError;
use crate::images::join_rel;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

const STYLE: &str = r#"body { font-family: "Songti SC", "Noto Serif CJK SC", "Palatino Linotype", serif; line-height: 1.7; margin: 1.2em; }
h1 { font-size: 1.55em; line-height: 1.3; }
h2 { font-size: 1.25em; }
img { max-width: 100%; height: auto; }
.meta { color: #555555; font-size: 0.92em; }
blockquote { margin-left: 0; padding-left: 0.8em; border-left: 3px solid #b9aa98; }
pre { white-space: pre-wrap; background: #f4f1ea; padding: 0.6em; }
code { font-family: Consolas, "Sarasa Mono SC", monospace; }
.placeholder { color: #666666; font-style: italic; }
table { border-collapse: collapse; width: 100%; }
td, th { border: 1px solid #ccc; padding: 0.3em 0.5em; vertical-align: top; }
"#;

pub fn write_epub(
    book: &BookDraft,
    output_dir: &Path,
    epub_path: &Path,
) -> Result<(), EngineError> {
    if let Some(parent) = epub_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = File::create(epub_path)?;
    let mut zip = ZipWriter::new(file);
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    zip.start_file("mimetype", stored)
        .map_err(|err| EngineError::Epub(err.to_string()))?;
    zip.write_all(b"application/epub+zip")?;

    write_entry(
        &mut zip,
        "META-INF/container.xml",
        deflated,
        CONTAINER.as_bytes(),
    )?;
    write_entry(&mut zip, "EPUB/style.css", deflated, STYLE.as_bytes())?;
    write_entry(
        &mut zip,
        "EPUB/nav.xhtml",
        deflated,
        nav_xhtml(book).as_bytes(),
    )?;

    let mut images: Vec<(String, String, &'static str)> = Vec::new();
    for chapter in &book.chapters {
        let name = format!("chapter-{:03}.xhtml", chapter.number);
        let xhtml = chapter_xhtml(chapter);
        collect_images(&xhtml, output_dir, &mut images);
        write_entry(
            &mut zip,
            &format!("EPUB/{name}"),
            deflated,
            xhtml.as_bytes(),
        )?;
    }
    images.sort();
    images.dedup();
    for (href, disk_name, media_type) in &images {
        let bytes = std::fs::read(join_rel(output_dir, disk_name))?;
        write_entry(&mut zip, &format!("EPUB/{href}"), deflated, &bytes)?;
        let _ = media_type;
    }
    write_entry(
        &mut zip,
        "EPUB/content.opf",
        deflated,
        content_opf(book, &images).as_bytes(),
    )?;
    zip.finish()
        .map_err(|err| EngineError::Epub(err.to_string()))?;
    Ok(())
}

fn write_entry(
    zip: &mut ZipWriter<File>,
    name: &str,
    options: SimpleFileOptions,
    bytes: &[u8],
) -> Result<(), EngineError> {
    zip.start_file(name, options)
        .map_err(|err| EngineError::Epub(err.to_string()))?;
    zip.write_all(bytes)?;
    Ok(())
}

fn chapter_xhtml(chapter: &crate::book::Chapter) -> String {
    let article = &chapter.article;
    let mut meta = format!(
        "<p class=\"meta\">来源：<a href=\"{}\">{}</a>",
        xml_escape(&article.source_url),
        xml_escape(&article.source_url)
    );
    if let Some(author) = article.author.as_deref().filter(|s| !s.is_empty()) {
        meta.push_str(&format!("<br/>作者：{}", xml_escape(author)));
    }
    if let Some(date) = article.published_at.as_deref().filter(|s| !s.is_empty()) {
        meta.push_str(&format!("<br/>时间：{}", xml_escape(date)));
    }
    meta.push_str("</p>\n");
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xml:lang="zh-CN" lang="zh-CN">
<head>
  <title>{title}</title>
  <link rel="stylesheet" type="text/css" href="style.css"/>
</head>
<body>
  <h1>{title}</h1>
  {meta}{body}
</body>
</html>
"#,
        title = xml_escape(&article.title),
        meta = meta,
        body = render_xhtml(&article.blocks)
    )
}

fn nav_xhtml(book: &BookDraft) -> String {
    let mut items = String::new();
    for chapter in &book.chapters {
        items.push_str(&format!(
            "<li><a href=\"chapter-{:03}.xhtml\">{}</a></li>\n",
            chapter.number,
            xml_escape(&chapter.article.title)
        ));
    }
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" lang="zh-CN">
<head><title>目录</title></head>
<body>
<nav epub:type="toc" id="toc">
  <h1>{}</h1>
  <ol>
    {items}
  </ol>
</nav>
</body>
</html>
"#,
        xml_escape(&book.title)
    )
}

fn content_opf(book: &BookDraft, images: &[(String, String, &'static str)]) -> String {
    let stamp = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
    let mut identity = String::new();
    identity.push_str(&book.title);
    identity.push('\n');
    for chapter in &book.chapters {
        identity.push_str(&chapter.article.source_url);
        identity.push('\n');
    }
    let id = hex::encode(Sha256::digest(identity.as_bytes()));
    let id = &id[..20];
    let mut manifest = String::from(
        r#"<item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="css" href="style.css" media-type="text/css"/>
"#,
    );
    let mut spine = String::new();
    for chapter in &book.chapters {
        let id = format!("c{:03}", chapter.number);
        let href = format!("chapter-{:03}.xhtml", chapter.number);
        manifest.push_str(&format!(
            "    <item id=\"{id}\" href=\"{href}\" media-type=\"application/xhtml+xml\"/>\n"
        ));
        spine.push_str(&format!("    <itemref idref=\"{id}\"/>\n"));
    }
    for (index, (href, _, media)) in images.iter().enumerate() {
        manifest.push_str(&format!(
            "    <item id=\"img{index}\" href=\"{}\" media-type=\"{media}\"/>\n",
            xml_escape(href)
        ));
    }
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="bookid" xml:lang="zh-CN">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="bookid">urn:articlebook:{id}</dc:identifier>
    <dc:title>{title}</dc:title>
    <dc:language>zh-CN</dc:language>
    <dc:creator>{author}</dc:creator>
    <meta property="dcterms:modified">{stamp}</meta>
  </metadata>
  <manifest>
    {manifest}
  </manifest>
  <spine>
    {spine}
  </spine>
</package>
"#,
        title = xml_escape(&book.title),
        author = xml_escape(&book.author),
    )
}

fn collect_images(xhtml: &str, output_dir: &Path, out: &mut Vec<(String, String, &'static str)>) {
    let mut rest = xhtml;
    while let Some(start) = rest.find("src=\"") {
        rest = &rest[start + 5..];
        let Some(end) = rest.find('"') else { break };
        let src = &rest[..end];
        rest = &rest[end..];
        if !src.starts_with("images/") {
            continue;
        }
        let disk = join_rel(output_dir, src);
        if !disk.is_file() {
            continue;
        }
        let media = match src.rsplit('.').next().unwrap_or("") {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "svg" => "image/svg+xml",
            _ => continue,
        };
        out.push((src.to_string(), src.to_string(), media));
    }
}

const CONTAINER: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="EPUB/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>
"#;

/// Read the stored mimetype entry. Used by tests and as a small sanity check.
pub fn epub_mimetype(path: &Path) -> Result<String, EngineError> {
    let file = File::open(path)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|err| EngineError::Epub(err.to_string()))?;
    let mut entry = archive
        .by_index(0)
        .map_err(|err| EngineError::Epub(err.to_string()))?;
    if entry.name() != "mimetype" {
        return Err(EngineError::Epub(format!(
            "EPUB 第一项应是 mimetype，实际是 {}",
            entry.name()
        )));
    }
    if entry.compression() != CompressionMethod::Stored {
        return Err(EngineError::Epub("mimetype 必须不压缩".into()));
    }
    let mut text = String::new();
    entry.read_to_string(&mut text)?;
    Ok(text)
}

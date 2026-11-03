use crate::convert::map_image_srcs;
use crate::fetch::HttpClient;
use crate::model::Article;
use crate::rate::Pace;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;

pub async fn localize_images(
    article: &mut Article,
    client: &dyn HttpClient,
    output_dir: &Path,
    pace: &Pace,
) -> Vec<String> {
    let mut warnings = Vec::new();
    let mut sources = Vec::new();
    collect_sources(&article.blocks, &mut sources);
    sources.sort();
    sources.dedup();
    let mut map = HashMap::new();
    for src in sources {
        if src.starts_with("images/") || src.starts_with("data:") {
            continue;
        }
        match download_image(client, &src, output_dir, pace).await {
            Ok(relative) => {
                map.insert(src, relative);
            }
            Err(err) => warnings.push(err),
        }
    }
    if !map.is_empty() {
        map_image_srcs(&mut article.blocks, &mut |src| {
            map.get(src).cloned().unwrap_or_else(|| src.to_string())
        });
    }
    warnings
}

fn collect_sources(blocks: &[crate::model::Block], out: &mut Vec<String>) {
    use crate::model::Block;
    for block in blocks {
        match block {
            Block::Heading { inlines, .. } | Block::Paragraph { inlines } => {
                collect_inline_sources(inlines, out);
            }
            Block::Quote { blocks } => collect_sources(blocks, out),
            Block::List { items, .. } => {
                for item in items {
                    collect_sources(item, out);
                }
            }
            Block::Image { src, .. } => out.push(src.clone()),
            Block::Table { rows } => {
                for row in rows {
                    for cell in row {
                        collect_inline_sources(cell, out);
                    }
                }
            }
            Block::Code { .. } | Block::Rule | Block::Placeholder { .. } => {}
        }
    }
}

fn collect_inline_sources(inlines: &[crate::model::Inline], out: &mut Vec<String>) {
    use crate::model::Inline;
    for inline in inlines {
        match inline {
            Inline::Image { src, .. } => out.push(src.clone()),
            Inline::Strong { children }
            | Inline::Em { children }
            | Inline::Link { children, .. } => {
                collect_inline_sources(children, out);
            }
            Inline::Text { .. } | Inline::Code { .. } | Inline::Break => {}
        }
    }
}

async fn download_image(
    client: &dyn HttpClient,
    src: &str,
    output_dir: &Path,
    pace: &Pace,
) -> Result<String, String> {
    pace.wait().await;
    let fetched = client
        .get(src)
        .await
        .map_err(|err| format!("图片未下载 {src}：{err}"))?;
    if !(200..300).contains(&fetched.status) {
        return Err(format!("图片未下载 {src}：HTTP {}", fetched.status));
    }
    let content_type = fetched.content_type.to_ascii_lowercase();
    if content_type.contains("text/html") || looks_like_html(&fetched.body) {
        return Err(format!("图片未下载 {src}：响应不是图片"));
    }
    let ext = image_ext(&fetched.body, &content_type, src)
        .ok_or_else(|| format!("图片未下载 {src}：无法识别格式"))?;
    if ext == "svg" {
        let text = String::from_utf8_lossy(&fetched.body).to_ascii_lowercase();
        if text.contains("<script") {
            return Err(format!("图片未下载 {src}：已跳过带脚本的 SVG"));
        }
    }
    if fetched.body.len() > 12 * 1024 * 1024 {
        return Err(format!("图片未下载 {src}：文件过大"));
    }
    let name = format!("{}.{ext}", &sha256_hex(src.as_bytes())[..16]);
    let dir = output_dir.join("images");
    std::fs::create_dir_all(&dir).map_err(|err| format!("无法创建图片目录：{err}"))?;
    std::fs::write(dir.join(&name), &fetched.body)
        .map_err(|err| format!("无法保存图片 {name}：{err}"))?;
    Ok(format!("images/{name}"))
}

fn looks_like_html(bytes: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(80)]).to_ascii_lowercase();
    head.contains("<!doctype html") || head.contains("<html")
}

fn image_ext(bytes: &[u8], content_type: &str, url: &str) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return Some("png");
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("jpg");
    }
    if bytes.starts_with(b"GIF8") {
        return Some("gif");
    }
    if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("webp");
    }
    if content_type.contains("png") {
        return Some("png");
    }
    if content_type.contains("jpeg") || content_type.contains("jpg") {
        return Some("jpg");
    }
    if content_type.contains("gif") {
        return Some("gif");
    }
    if content_type.contains("webp") {
        return Some("webp");
    }
    if content_type.contains("svg") {
        return Some("svg");
    }
    let path = url::Url::parse(url)
        .ok()
        .map(|url| url.path().to_ascii_lowercase())
        .unwrap_or_default();
    if path.ends_with(".png") {
        Some("png")
    } else if path.ends_with(".jpg") || path.ends_with(".jpeg") {
        Some("jpg")
    } else if path.ends_with(".gif") {
        Some("gif")
    } else if path.ends_with(".webp") {
        Some("webp")
    } else if path.ends_with(".svg") {
        Some("svg")
    } else {
        None
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub fn join_rel(base: &Path, relative: &str) -> std::path::PathBuf {
    relative
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .fold(base.to_path_buf(), |path, part| path.join(part))
}

use crate::book::draft_from_articles;
use crate::epub::{epub_mimetype, write_epub};
use crate::extract::{extract_html, source_hint_from_html};
use crate::fetch::{Fetched, HttpClient, USER_AGENT};
use crate::history::{save_session, session_from_page_url, HistoryClient};
use crate::pipeline::{export_urls, ExportOptions, NullSink};
use crate::provider::{
    describe_providers, provider_by_id, AccountProvider, AccountRequest, WechatWebProvider,
};
use crate::urlutil::{normalize_url, parse_url_list, NormalizeOptions};
use crate::EngineError;
use async_trait::async_trait;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

fn scratch() -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let id = N.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "articlebook-{}-{}-{id}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn normalizes_tracking_params_and_host() {
    let opts = NormalizeOptions::default();
    let url = normalize_url("HTTPS://Example.COM/a/?utm_source=x&b=2&a=1#frag", &opts).unwrap();
    assert_eq!(url, "https://example.com/a?a=1&b=2");
    let again = normalize_url(&url, &opts).unwrap();
    assert_eq!(again, url);
}

#[test]
fn wechat_keeps_identity_and_drops_share_params() {
    let url = normalize_url(
        "https://mp.weixin.qq.com/s?__biz=MzA&mid=1&idx=2&sn=abc&scene=21&sharer_shareid=9&chksm=ff",
        &NormalizeOptions::default(),
    )
    .unwrap();
    assert!(url.contains("__biz=MzA"), "{url}");
    assert!(url.contains("mid=1"), "{url}");
    assert!(url.contains("idx=2"), "{url}");
    assert!(url.contains("sn=abc"), "{url}");
    assert!(!url.contains("scene="), "{url}");
    assert!(!url.contains("sharer_shareid"), "{url}");
    assert!(!url.contains("chksm"), "{url}");
    assert_eq!(
        url,
        "https://mp.weixin.qq.com/s?__biz=MzA&idx=2&mid=1&sn=abc"
    );
}

#[test]
fn parse_url_list_dedupes_and_skips_notes() {
    let text = "\
https://Example.com/a?utm_source=newsletter
https://example.com/a
# 这是注释 https://example.com/skip
请看这篇 https://example.com/posts/two。
not-a-url
";
    let parsed = parse_url_list(text, &NormalizeOptions::default());
    assert_eq!(
        parsed.urls,
        vec![
            "https://example.com/a".to_string(),
            "https://example.com/posts/two".to_string(),
        ]
    );
    assert_eq!(parsed.duplicates, 1);
    assert_eq!(parsed.invalid, vec!["not-a-url".to_string()]);
}

#[test]
fn extracts_wechat_fixture() {
    let html = fixture("wechat_sample.html");
    let hinted = source_hint_from_html(&html).unwrap();
    assert_eq!(hinted, "https://mp.weixin.qq.com/s/demo-article");
    let article = extract_html(&html, &hinted).unwrap();
    assert_eq!(article.title, "把长文收成一本小书");
    assert_eq!(article.author.as_deref(), Some("ArticleBook 示例"));
    assert_eq!(article.published_at.as_deref(), Some("2026-10-01"));
    assert!(matches!(article.kind, crate::ArticleKind::Wechat));
    let markdown = crate::render_markdown(&article.blocks);
    assert!(
        markdown.contains("这是一篇用于离线测试的微信文章正文"),
        "{markdown}"
    );
    assert!(markdown.contains("**本地化图片**"), "{markdown}");
    assert!(markdown.contains("https://example.com/read"), "{markdown}");
    assert!(
        markdown.contains("https://mmbiz.qpic.cn/example/cover.jpg"),
        "{markdown}"
    );
    assert!(markdown.contains("封面"), "{markdown}");
    assert!(markdown.contains("互动组件：投票"), "{markdown}");
    assert!(!markdown.contains("二维码不该出现"), "{markdown}");
    assert!(!markdown.contains("推荐阅读不该出现"), "{markdown}");
    assert!(!markdown.contains("og 标题不该优先"), "{markdown}");
}

#[test]
fn extracts_generic_fixture_without_chrome() {
    let html = fixture("generic_sample.html");
    let article = extract_html(&html, "https://example.com/posts/weekend-reading").unwrap();
    assert_eq!(article.title, "周末把收藏夹读完");
    assert_eq!(article.author.as_deref(), Some("林深"));
    assert_eq!(
        article.published_at.as_deref(),
        Some("2026-09-12T08:00:00Z")
    );
    assert!(matches!(article.kind, crate::ArticleKind::Generic));
    let markdown = crate::render_markdown(&article.blocks);
    assert!(markdown.contains("真正要留下的是这一段正文"), "{markdown}");
    assert!(markdown.contains("为什么要归档"), "{markdown}");
    assert!(markdown.contains("链接列表"), "{markdown}");
    assert!(markdown.contains("手动目录"), "{markdown}");
    assert!(markdown.contains("只保存自己有权阅读的内容"), "{markdown}");
    assert!(markdown.contains("articlebook batch"), "{markdown}");
    assert!(!markdown.contains("首页导航不该出现"), "{markdown}");
    assert!(!markdown.contains("相关文章不该出现"), "{markdown}");
    assert!(!markdown.contains("版权页脚不该出现"), "{markdown}");
}

#[test]
fn generic_extractor_finds_a_div_when_there_is_no_article_tag() {
    let html = r#"<!doctype html><html><body>
      <div class="site-nav">首页导航不该出现，即使这段导航写得很长也不该进正文。</div>
      <div class="content">
        <h1>无 article 标签</h1>
        <p>第一段足够长的正文，用来确认通用提取会选中内容容器而不是页眉。</p>
        <p>第二段继续写一点内容，让段落分数明显高于导航。</p>
      </div>
      <div class="site-footer">版权页脚不该出现</div>
    </body></html>"#;
    let article = extract_html(html, "https://example.com/div-only").unwrap();
    assert_eq!(article.title, "无 article 标签");
    let markdown = crate::render_markdown(&article.blocks);
    assert!(markdown.contains("第一段足够长的正文"), "{markdown}");
    assert!(!markdown.contains("首页导航不该出现"), "{markdown}");
    assert!(!markdown.contains("版权页脚不该出现"), "{markdown}");
}

#[test]
fn wechat_verify_page_is_an_error() {
    let html = "<html><body><p>环境异常，请去验证后再访问</p></body></html>";
    let err = extract_html(html, "https://mp.weixin.qq.com/s/blocked").unwrap_err();
    let text = err.to_string();
    assert!(text.contains("验证") || text.contains("正文"), "{text}");
}

#[test]
fn assemble_orders_chapters_and_writes_toc() {
    let first = extract_html(
        &fixture("wechat_sample.html"),
        "https://mp.weixin.qq.com/s/demo-article",
    )
    .unwrap();
    let second = extract_html(
        &fixture("generic_sample.html"),
        "https://example.com/posts/weekend-reading",
    )
    .unwrap();
    let draft = draft_from_articles(
        "样例书".into(),
        "个人归档".into(),
        vec![(1, first), (2, second)],
    );
    let markdown = crate::render_book_markdown(&draft);
    assert!(markdown.contains("# 样例书"), "{markdown}");
    assert!(
        markdown.contains("[把长文收成一本小书](#article-1)"),
        "{markdown}"
    );
    assert!(
        markdown.contains("[周末把收藏夹读完](#article-2)"),
        "{markdown}"
    );
    let wechat_at = markdown.find("把长文收成一本小书").unwrap();
    let generic_at = markdown.find("周末把收藏夹读完").unwrap();
    assert!(wechat_at < generic_at);
    assert!(markdown.contains("这是一篇用于离线测试的微信文章正文"));
    assert!(markdown.contains("真正要留下的是这一段正文"));
    assert!(!markdown.contains("推荐阅读不该出现"));
    assert!(!markdown.contains("首页导航不该出现"));

    let dir = scratch();
    crate::write_book(&draft, &dir).unwrap();
    assert!(dir.join("book.md").is_file());
    assert!(dir.join("articles/001.md").is_file());
    assert!(dir.join("articles/002.md").is_file());
    let epub = dir.join("book.epub");
    write_epub(&draft, &dir, &epub).unwrap();
    assert_eq!(epub_mimetype(&epub).unwrap(), "application/epub+zip");
    let file = std::fs::File::open(&epub).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    let mut names = Vec::new();
    for index in 0..archive.len() {
        names.push(archive.by_index(index).unwrap().name().to_string());
    }
    assert!(
        names.iter().any(|name| name == "EPUB/content.opf"),
        "{names:?}"
    );
    assert!(
        names.iter().any(|name| name.contains("chapter-001")),
        "{names:?}"
    );
    let mut opf = String::new();
    archive
        .by_name("EPUB/content.opf")
        .unwrap()
        .read_to_string(&mut opf)
        .unwrap();
    assert!(opf.contains("样例书"), "{opf}");
    let _ = std::fs::remove_dir_all(&dir);
}

struct ScriptedClient {
    pages: Mutex<HashMap<String, Vec<Result<Fetched, String>>>>,
    hits: AtomicUsize,
}

impl ScriptedClient {
    fn new(pages: HashMap<String, Vec<Result<Fetched, String>>>) -> Self {
        Self {
            pages: Mutex::new(pages),
            hits: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl HttpClient for ScriptedClient {
    async fn get(&self, url: &str) -> Result<Fetched, EngineError> {
        self.hits.fetch_add(1, Ordering::SeqCst);
        let mut pages = self.pages.lock().expect("pages");
        let seq = pages
            .get_mut(url)
            .ok_or_else(|| EngineError::Fetch(format!("测试客户端没有 {url}")))?;
        if seq.is_empty() {
            return Err(EngineError::Fetch("没有更多响应".into()));
        }
        if seq.len() == 1 {
            return seq[0].clone().map_err(EngineError::Fetch);
        }
        seq.remove(0).map_err(EngineError::Fetch)
    }
}

fn html_page(url: &str, title: &str, body: &str) -> Fetched {
    let html = format!(
        "<!doctype html><html><head><title>{title}</title></head><body><article><h1>{title}</h1><p>{body}</p></article></body></html>"
    );
    Fetched {
        status: 200,
        content_type: "text/html; charset=utf-8".into(),
        body: html.into_bytes(),
        final_url: url.into(),
    }
}

fn opts(dir: &Path) -> ExportOptions {
    let mut opts = ExportOptions::new(dir.join("out"), dir.join("cache.db"));
    opts.book_title = "测试书".into();
    opts.book_author = "测试".into();
    opts.delay_ms = 0;
    opts.retries = 1;
    opts.concurrency = 2;
    opts.export_epub = true;
    opts.use_cache = true;
    opts
}

#[tokio::test]
async fn pipeline_skips_failures_and_keeps_order() {
    let dir = scratch();
    let ok = "https://example.com/one";
    let bad = "https://example.com/missing";
    let third = "https://example.com/three";
    let mut pages = HashMap::new();
    pages.insert(ok.into(), vec![Ok(html_page(
        ok,
        "第一篇",
        "第一篇正文足够长，用来通过提取阈值，确认批处理会写进书里。这里再补一段，避免被误判成没有正文。",
    ))]);
    pages.insert(
        bad.into(),
        vec![Ok(Fetched {
            status: 404,
            content_type: "text/html".into(),
            body: b"missing".to_vec(),
            final_url: bad.into(),
        })],
    );
    pages.insert(
        third.into(),
        vec![Ok(html_page(
            third,
            "第三篇",
            "第三篇正文也足够长，应该排在成功章节的第二位。再写一句，让段落明显长于导航和页脚。",
        ))],
    );
    let client = Arc::new(ScriptedClient::new(pages));
    let report = export_urls(
        &format!("{ok}\n{bad}\n{third}\n"),
        opts(&dir),
        client,
        Arc::new(NullSink),
    )
    .await
    .unwrap();
    assert_eq!(report.succeeded, 2);
    assert_eq!(report.failed, 1);
    assert_eq!(report.failures[0].url, bad);
    let book = std::fs::read_to_string(dir.join("out/book.md")).unwrap();
    assert!(book.contains("第一篇"));
    assert!(book.contains("第三篇"));
    assert!(book.find("第一篇").unwrap() < book.find("第三篇").unwrap());
    assert!(!book.contains("missing"));
    assert!(dir.join("out/book.epub").is_file());
    assert!(dir.join("out/failures.md").is_file());
    assert_eq!(
        epub_mimetype(&dir.join("out/book.epub")).unwrap(),
        "application/epub+zip"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn pipeline_retries_then_succeeds_and_cache_skips_second_fetch() {
    let dir = scratch();
    let url = "https://example.com/retry";
    let page = html_page(
        url,
        "重试后的文章",
        "这段正文足够长，确认失败一次之后仍会进入书籍。补充一句，让通用提取稳定选中 article。",
    );
    let mut pages = HashMap::new();
    pages.insert(url.into(), vec![Err("临时网络错误".into()), Ok(page)]);
    let client = Arc::new(ScriptedClient::new(pages));
    let mut options = opts(&dir);
    options.retries = 1;
    let text = format!("{url}\n");
    let report = export_urls(&text, options, client.clone(), Arc::new(NullSink))
        .await
        .unwrap();
    assert_eq!(report.succeeded, 1, "{:?}", report.failures);
    assert_eq!(client.hits.load(Ordering::SeqCst), 2);
    let mut options = opts(&dir);
    options.output_dir = dir.join("out-2");
    options.retries = 0;
    let second = export_urls(&text, options, client.clone(), Arc::new(NullSink))
        .await
        .unwrap();
    assert_eq!(second.cached, 1);
    assert_eq!(client.hits.load(Ordering::SeqCst), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn pipeline_localizes_images_into_the_book() {
    let dir = scratch();
    let page_url = "https://example.com/with-image";
    let image_url = "https://images.test/a.png";
    let png = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pixel.png"))
        .unwrap();
    let html = format!(
        "<!doctype html><html><head><title>带图的笔记</title></head><body><article><h1>带图的笔记</h1><p>这段正文足够长，用来确认图片会被下载到本地目录。再补一句，避免正文太短。</p><img src=\"{image_url}\" alt=\"配图\"/></article></body></html>"
    );
    let mut pages = HashMap::new();
    pages.insert(
        page_url.into(),
        vec![Ok(Fetched {
            status: 200,
            content_type: "text/html".into(),
            body: html.into_bytes(),
            final_url: page_url.into(),
        })],
    );
    pages.insert(
        image_url.into(),
        vec![Ok(Fetched {
            status: 200,
            content_type: "image/png".into(),
            body: png,
            final_url: image_url.into(),
        })],
    );
    let client = Arc::new(ScriptedClient::new(pages));
    let mut options = opts(&dir);
    options.localize_images = true;
    let report = export_urls(
        &format!("{page_url}\n"),
        options,
        client,
        Arc::new(NullSink),
    )
    .await
    .unwrap();
    assert_eq!(report.succeeded, 1, "{:?}", report.failures);
    let book = std::fs::read_to_string(&report.book_markdown).unwrap();
    assert!(book.contains("images/"), "{book}");
    assert!(!book.contains(image_url), "{book}");
    let images = std::fs::read_dir(dir.join("out/images")).unwrap();
    assert!(images.count() >= 1);
    let article = std::fs::read_to_string(dir.join("out/articles/001.md")).unwrap();
    assert!(article.contains("../images/"), "{article}");
    let _ = std::fs::remove_dir_all(&dir);
}

fn wait<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(future)
}

fn blank_account(name: &str, links: &str, file: Option<PathBuf>) -> AccountRequest {
    AccountRequest {
        account_id: String::new(),
        account_name: name.into(),
        links_text: links.into(),
        links_file: file,
        session_file: None,
        history_limit: 0,
    }
}

#[test]
fn manual_provider_reads_a_links_file_and_wmpf_stays_unavailable() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-links.txt");
    let listing = wait(provider_by_id("manual").unwrap().list_links(&blank_account(
        "示例合集",
        "",
        Some(path),
    )))
    .unwrap();
    assert_eq!(listing.account_name, "示例合集");
    assert!(listing.urls_text.contains("https://example.com/posts/one"));
    let parsed = parse_url_list(&listing.urls_text, &NormalizeOptions::default());
    assert_eq!(parsed.duplicates, 1);
    assert!(parsed.urls.iter().any(|url| url.contains("__biz=MzTest")));
    assert!(parsed.urls.iter().all(|url| !url.contains("scene=")));
    let err = wait(provider_by_id("wmpf").unwrap().list_links(&blank_account(
        "",
        "https://example.com",
        None,
    )))
    .unwrap_err();
    assert!(err.to_string().contains("WMPF"), "{err}");
    let described = describe_providers();
    assert!(described
        .iter()
        .any(|item| item.id == "wechat-web" && item.available));
    assert!(described
        .iter()
        .any(|item| item.id == "manual" && item.available));
    assert!(described
        .iter()
        .any(|item| item.id == "wmpf" && !item.available && item.note.contains("Frida")));
    let missing = wait(
        provider_by_id("wechat-web")
            .unwrap()
            .list_links(&blank_account("", "", None)),
    )
    .unwrap_err();
    assert!(missing.to_string().contains("会话"), "{missing}");
}

#[test]
fn windows_workflow_ships_windows_only() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows/build-windows.yml");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
    assert!(text.contains("windows-latest"));
    assert!(text.contains("workflow_dispatch"));
    let lower = text.to_ascii_lowercase();
    assert!(!lower.contains("macos"));
    assert!(!lower.contains("ubuntu"));
}

struct SeqHistory {
    bodies: Mutex<Vec<String>>,
    seen: Mutex<Vec<(String, String, String)>>,
}

#[async_trait]
impl HistoryClient for SeqHistory {
    async fn get_text(
        &self,
        url: &str,
        cookie: &str,
        user_agent: &str,
        referer: &str,
    ) -> Result<String, EngineError> {
        assert!(url.starts_with("https://mp.weixin.qq.com/"), "{url}");
        assert_eq!(user_agent, USER_AGENT);
        assert!(!user_agent.contains("MicroMessenger"));
        assert!(referer.starts_with("https://mp.weixin.qq.com/"));
        self.seen.lock().expect("seen").push((
            url.to_string(),
            cookie.to_string(),
            referer.to_string(),
        ));
        let mut bodies = self.bodies.lock().expect("bodies");
        if bodies.is_empty() {
            return Err(EngineError::Fetch("没有更多目录页".into()));
        }
        Ok(bodies.remove(0))
    }
}

fn wechat_html(title: &str) -> String {
    format!(
        "<div id=\"activity-name\">{title}</div><div id=\"js_name\">示例</div><div id=\"js_content\"><p>这是一篇用于离线测试的微信文章正文，不应该依赖网络。再补一句让正文长度稳定通过提取。</p></div>"
    )
}

#[tokio::test]
async fn wechat_web_history_fixture_lists_then_exports() {
    let dir = scratch();
    let session = session_from_page_url(
        "https://mp.weixin.qq.com/mp/profile_ext?action=home&__biz=MzFixture&uin=777&key=abc&pass_ticket=pt&appmsg_token=amt",
        "wap_sid2=test",
        "示例号",
    )
    .unwrap();
    let session_path = dir.join("wechat-web-session.json");
    save_session(&session_path, &session).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&session_path)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o077,
            0,
            "session file should not be group/world readable"
        );
    }
    let stored = std::fs::read_to_string(&session_path).unwrap();
    stored_contains_cookie(&stored);
    let client = Arc::new(SeqHistory {
        bodies: Mutex::new(vec![
            fixture("wechat_history_getmsg.json"),
            fixture("wechat_history_getmsg_page2.json"),
        ]),
        seen: Mutex::new(Vec::new()),
    });
    let provider = WechatWebProvider::with_client(
        Arc::clone(&client) as Arc<dyn HistoryClient>,
        std::time::Duration::ZERO,
    );
    let mut request = blank_account("", "", None);
    request.session_file = Some(session_path);
    request.history_limit = 20;
    let listing = provider.list_links(&request).await.unwrap();
    assert_eq!(listing.account_name, "示例号");
    assert!(listing.source_note.contains("历史消息"));
    assert!(!listing.source_note.contains("wap_sid2"));
    let urls = parse_url_list(&listing.urls_text, &NormalizeOptions::default()).urls;
    assert_eq!(urls.len(), 3, "{urls:?}");
    let seen = client.seen.lock().expect("seen");
    assert_eq!(seen.len(), 2);
    assert!(seen[0].0.contains("offset=0"));
    assert!(seen[1].0.contains("offset=1"), "{}", seen[1].0);
    assert_eq!(seen[0].1, "wap_sid2=test");
    drop(seen);

    let mut pages = HashMap::new();
    let titles = ["把长文收成一本小书", "第二篇", "周末把收藏夹读完"];
    for (url, title) in urls.iter().zip(titles) {
        pages.insert(
            url.clone(),
            vec![Ok(Fetched {
                status: 200,
                content_type: "text/html".into(),
                body: wechat_html(title).into_bytes(),
                final_url: url.clone(),
            })],
        );
    }
    let report = export_urls(
        &listing.urls_text,
        opts(&dir),
        Arc::new(ScriptedClient::new(pages)),
        Arc::new(NullSink),
    )
    .await
    .unwrap();
    assert_eq!(report.succeeded, 3, "{:?}", report.failures);
    let book = std::fs::read_to_string(&report.book_markdown).unwrap();
    assert!(book.contains("把长文收成一本小书"), "{book}");
    assert!(book.contains("第二篇"), "{book}");
    assert!(book.contains("周末把收藏夹读完"), "{book}");
    assert!(book.find("把长文收成一本小书").unwrap() < book.find("第二篇").unwrap());
    assert!(book.find("第二篇").unwrap() < book.find("周末把收藏夹读完").unwrap());
    let _ = std::fs::remove_dir_all(&dir);
}

fn stored_contains_cookie(stored: &str) {
    assert!(stored.contains("wap_sid2=test"), "{stored}");
}

fn reader_history_page(start: usize, count: usize, can_continue: bool) -> String {
    let list: Vec<_> = (0..count)
        .map(|i| {
            let n = start + i;
            serde_json::json!({
                "comm_msg_info": {"type": 49, "id": n},
                "app_msg_ext_info": {
                    "title": format!("文章{n}"),
                    "content_url": format!(
                        "https://mp.weixin.qq.com/s?__biz=MzFixture&mid={n}&idx=1&sn=s{n}"
                    )
                }
            })
        })
        .collect();
    serde_json::json!({
        "ret": 0,
        "errmsg": "ok",
        "msg_count": count,
        "can_msg_continue": if can_continue { 1 } else { 0 },
        "general_msg_list": serde_json::json!({ "list": list }).to_string()
    })
    .to_string()
}

#[tokio::test]
async fn wechat_web_history_limit_zero_reads_past_two_hundred() {
    let dir = scratch();
    let session = session_from_page_url(
        "https://mp.weixin.qq.com/mp/profile_ext?action=home&__biz=MzFixture&key=abc&appmsg_token=amt",
        "wap_sid2=test",
        "示例号",
    )
    .unwrap();
    let session_path = dir.join("wechat-web-session.json");
    save_session(&session_path, &session).unwrap();
    let pages = 21;
    let per_page = 10;
    let client = Arc::new(SeqHistory {
        bodies: Mutex::new(
            (0..pages)
                .map(|page| reader_history_page(page * per_page, per_page, page + 1 != pages))
                .collect(),
        ),
        seen: Mutex::new(Vec::new()),
    });
    let provider = WechatWebProvider::with_client(
        Arc::clone(&client) as Arc<dyn HistoryClient>,
        std::time::Duration::ZERO,
    );
    let mut request = blank_account("示例号", "", None);
    request.session_file = Some(session_path);
    request.history_limit = 0;
    let listing = provider.list_links(&request).await.unwrap();
    let urls = parse_url_list(&listing.urls_text, &NormalizeOptions::default()).urls;
    assert_eq!(urls.len(), 210, "history_limit 0 must keep paging past 200");
    assert!(listing.source_note.contains("210"));
    assert!(!listing.source_note.contains("安全上限"));
    assert_eq!(client.seen.lock().expect("seen").len(), pages);
    let _ = std::fs::remove_dir_all(&dir);
}

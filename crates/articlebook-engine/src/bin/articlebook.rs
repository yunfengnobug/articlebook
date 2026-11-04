use articlebook_engine::{
    cli_app_dirs, export_urls, extract_html, init_logging, normalize_url, parse_history_document,
    source_hint_from_html, suggest_output_dir, today_stamp, ExportOptions, NormalizeOptions,
    NullSink, ReqwestClient, USER_AGENT,
};
use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

fn main() {
    let code = match run() {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("错误：{err}");
            1
        }
    };
    std::process::exit(code);
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        print_help();
        return Ok(());
    }
    let command = args.remove(0);
    match command.as_str() {
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        "paths" => {
            let dirs = cli_app_dirs().map_err(|err| err.to_string())?;
            println!("data {}", dirs.data_dir.display());
            println!("cache {}", dirs.cache_dir.display());
            println!("logs {}", dirs.log_dir.display());
            println!("suggest {}", suggest_output_dir().display());
            Ok(())
        }
        "normalize" => {
            let raw = args
                .first()
                .ok_or("用法：articlebook-cli normalize <url>")?;
            let url =
                normalize_url(raw, &NormalizeOptions::default()).map_err(|err| err.to_string())?;
            println!("{url}");
            Ok(())
        }
        "extract" => {
            let path = args
                .first()
                .ok_or("用法：articlebook-cli extract <file.html>")?;
            let html = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
            let url = source_hint_from_html(&html).unwrap_or_else(|| format!("file://{path}"));
            let article = extract_html(&html, &url).map_err(|err| err.to_string())?;
            println!("# {}", article.title);
            if let Some(author) = &article.author {
                println!("- 作者：{author}");
            }
            println!("- 来源：{}", article.source_url);
            println!();
            print!("{}", articlebook_engine::render_markdown(&article.blocks));
            Ok(())
        }
        "assemble-html" => assemble_html(&args),
        "batch" => batch(&args),
        "history-parse" => history_parse(&args),
        other => Err(format!(
            "未知命令：{other}。运行 articlebook-cli help 查看用法。"
        )),
    }
}

fn assemble_html(args: &[String]) -> Result<(), String> {
    let out = flag(args, "--out").ok_or("缺少 --out")?;
    let title = flag(args, "--title").unwrap_or_else(|| format!("阅读归档 {}", today_stamp()));
    let author = flag(args, "--author").unwrap_or_else(|| "个人归档".into());
    let files = positionals(args);
    if files.is_empty() {
        return Err("请传入至少一个 HTML 文件".into());
    }
    let mut articles = Vec::new();
    for (index, path) in files.iter().enumerate() {
        let html = std::fs::read_to_string(path).map_err(|err| format!("{path}：{err}"))?;
        let url = source_hint_from_html(&html).unwrap_or_else(|| format!("file://{path}"));
        let article = extract_html(&html, &url).map_err(|err| format!("{path}：{err}"))?;
        articles.push((index + 1, article));
    }
    let draft = articlebook_engine::draft_from_articles(title, author, articles);
    let out_dir = PathBuf::from(out);
    articlebook_engine::write_book(&draft, &out_dir).map_err(|err| err.to_string())?;
    if !args.iter().any(|arg| arg == "--no-epub") {
        let epub = out_dir.join("book.epub");
        articlebook_engine::write_epub(&draft, &out_dir, &epub).map_err(|err| err.to_string())?;
        println!("epub {}", epub.display());
    }
    println!("book {}", out_dir.join("book.md").display());
    Ok(())
}

fn batch(args: &[String]) -> Result<(), String> {
    let input = flag(args, "--input").ok_or("缺少 --input")?;
    let out = flag(args, "--out").ok_or("缺少 --out")?;
    let dirs = cli_app_dirs().map_err(|err| err.to_string())?;
    init_logging(&dirs.log_dir).map_err(|err| err.to_string())?;
    let text = std::fs::read_to_string(&input).map_err(|err| format!("{input}：{err}"))?;
    let mut opts = ExportOptions::new(PathBuf::from(out), dirs.cache_dir.join("articles.db"));
    opts.book_title =
        flag(args, "--title").unwrap_or_else(|| format!("阅读归档 {}", today_stamp()));
    opts.book_author = flag(args, "--author").unwrap_or_else(|| "个人归档".into());
    opts.export_epub = !args.iter().any(|arg| arg == "--no-epub");
    opts.localize_images = args.iter().any(|arg| arg == "--images");
    opts.use_cache = !args.iter().any(|arg| arg == "--no-cache");
    if let Some(value) = flag(args, "--concurrency") {
        let parsed: usize = value.parse().map_err(|_| "--concurrency 不是数字")?;
        opts.concurrency = parsed.clamp(1, 8);
    }
    if let Some(value) = flag(args, "--retries") {
        opts.retries = value.parse().map_err(|_| "--retries 不是数字")?;
    }
    if let Some(value) = flag(args, "--delay") {
        opts.delay_ms = value.parse().map_err(|_| "--delay 不是数字")?;
    }
    let client = Arc::new(
        ReqwestClient::new(USER_AGENT, Duration::from_secs(opts.timeout_secs))
            .map_err(|err| err.to_string())?,
    );
    let report = runtime()
        .block_on(export_urls(&text, opts, client, Arc::new(NullSink)))
        .map_err(|err| err.to_string())?;
    println!(
        "成功 {}，失败 {}，缓存 {}",
        report.succeeded, report.failed, report.cached
    );
    println!("book {}", report.book_markdown);
    if let Some(epub) = &report.book_epub {
        println!("epub {epub}");
    }
    println!("failures {}", report.failures_markdown);
    for warning in &report.warnings {
        println!("注意：{warning}");
    }
    Ok(())
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|arg| arg == name).and_then(|index| {
        args.get(index + 1)
            .filter(|value| !value.starts_with("--"))
            .cloned()
    })
}

fn positionals(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut skip = false;
    for arg in args {
        if skip {
            skip = false;
            continue;
        }
        if matches!(
            arg.as_str(),
            "--out"
                | "--title"
                | "--author"
                | "--input"
                | "--concurrency"
                | "--retries"
                | "--delay"
        ) {
            skip = true;
            continue;
        }
        if arg.starts_with("--") {
            continue;
        }
        out.push(arg.clone());
    }
    out
}

fn history_parse(args: &[String]) -> Result<(), String> {
    let path = args
        .first()
        .filter(|value| !value.starts_with("--"))
        .ok_or("用法：articlebook-cli history-parse <history.json>")?;
    let body = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
    let entries = parse_history_document(&body).map_err(|err| err.to_string())?;
    for entry in &entries {
        println!("{}", entry.url);
    }
    eprintln!("解析到 {} 篇", entries.len());
    Ok(())
}

fn print_help() {
    println!(
        "\
ArticleBook 命令行（个人阅读归档）

  articlebook-cli paths
  articlebook-cli normalize <url>
  articlebook-cli extract <file.html>
  articlebook-cli assemble-html --out DIR --title TITLE file.html ...
  articlebook-cli batch --input urls.txt --out DIR [--title TITLE] [--images] [--no-epub] [--no-cache]
  articlebook-cli history-parse fixtures/wechat_history_getmsg.json

只归档你有权保存的链接。history-parse 只解析离线目录样例，不会登录。
桌面版的公众号模式用你自己打开的历史消息页或公众平台图文列表，默认拉取该号的全部历史（单次最多 5000 篇、1000 页）。
不会调用 WMPF / Frida，也不会伪装微信客户端。
数据目录：CLI 使用系统的 ArticleBook 数据/缓存目录；桌面版使用 Tauri 应用目录。
"
    );
}

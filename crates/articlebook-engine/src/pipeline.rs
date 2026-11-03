use crate::book::{draft_from_articles, render_failures, write_book, BookDraft};
use crate::cache::ArticleCache;
use crate::epub::write_epub;
use crate::error::EngineError;
use crate::extract::extract_html;
use crate::fetch::{decode_document, HttpClient};
use crate::images::localize_images;
use crate::model::{Article, FailureRecord};
use crate::rate::Pace;
use crate::urlutil::{parse_url_list, NormalizeOptions};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Semaphore;
use tracing::info;

#[derive(Debug, Clone)]
pub struct ExportOptions {
    pub output_dir: PathBuf,
    pub book_title: String,
    pub book_author: String,
    pub localize_images: bool,
    pub export_epub: bool,
    pub strip_tracking: bool,
    pub concurrency: usize,
    pub retries: u32,
    pub delay_ms: u64,
    pub use_cache: bool,
    pub cache_db: PathBuf,
    pub timeout_secs: u64,
}

impl ExportOptions {
    pub fn new(output_dir: PathBuf, cache_db: PathBuf) -> Self {
        Self {
            output_dir,
            book_title: "阅读归档".into(),
            book_author: "个人归档".into(),
            localize_images: false,
            export_epub: true,
            strip_tracking: true,
            concurrency: 2,
            retries: 2,
            delay_ms: 600,
            use_cache: true,
            cache_db,
            timeout_secs: 20,
        }
    }

    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_secs.max(5))
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArticleSummary {
    pub title: String,
    pub url: String,
    pub cached: bool,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobReport {
    pub job_id: String,
    pub output_dir: String,
    pub book_markdown: String,
    pub book_epub: Option<String>,
    pub failures_markdown: String,
    pub succeeded: usize,
    pub failed: usize,
    pub cached: usize,
    pub failures: Vec<FailureRecord>,
    pub articles: Vec<ArticleSummary>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    pub job_id: String,
    pub phase: String,
    pub index: usize,
    pub total: usize,
    pub url: String,
    pub title: Option<String>,
    pub status: String,
    pub message: String,
    pub attempt: u32,
}

pub trait ProgressSink: Send + Sync {
    fn emit(&self, event: ProgressEvent);
}

pub struct NullSink;

impl ProgressSink for NullSink {
    fn emit(&self, _event: ProgressEvent) {}
}

struct OneResult {
    index: usize,
    url: String,
    outcome: Outcome,
    warnings: Vec<String>,
}

enum Outcome {
    Ready { article: Article, cached: bool },
    Failed { error: String, attempts: u32 },
}

pub async fn export_urls(
    urls_text: &str,
    opts: ExportOptions,
    client: Arc<dyn HttpClient>,
    sink: Arc<dyn ProgressSink>,
) -> Result<JobReport, EngineError> {
    let job_id = next_job_id();
    let parsed = parse_url_list(
        urls_text,
        &NormalizeOptions {
            strip_tracking: opts.strip_tracking,
            extra_strip: Vec::new(),
            keep_fragment: false,
        },
    );
    if parsed.urls.is_empty() {
        return Err(EngineError::Message(if parsed.invalid.is_empty() {
            "没有解析到可用链接".into()
        } else {
            format!("没有解析到可用链接，已跳过 {} 行", parsed.invalid.len())
        }));
    }

    std::fs::create_dir_all(&opts.output_dir)?;
    std::fs::create_dir_all(opts.output_dir.join("articles"))?;
    let cache = Arc::new(tokio::sync::Mutex::new(ArticleCache::open(&opts.cache_db)?));
    let pace = Arc::new(Pace::new(Duration::from_millis(opts.delay_ms)));
    let limit = opts.concurrency.clamp(1, 8);
    let semaphore = Arc::new(Semaphore::new(limit));
    let total = parsed.urls.len();
    info!(%job_id, total, "开始归档");

    let mut tasks = Vec::with_capacity(total);
    for (index, url) in parsed.urls.iter().cloned().enumerate() {
        let client = client.clone();
        let sink = sink.clone();
        let cache = cache.clone();
        let pace = pace.clone();
        let semaphore = semaphore.clone();
        let output_dir = opts.output_dir.clone();
        let job_id = job_id.clone();
        let use_cache = opts.use_cache;
        let retries = opts.retries;
        let localize = opts.localize_images;
        tasks.push(tokio::spawn(async move {
            let _permit = semaphore.acquire_owned().await.expect("semaphore");
            process_one(
                job_id, index, total, url, client, sink, cache, pace, use_cache, retries, localize,
                output_dir,
            )
            .await
        }));
    }

    let mut results = Vec::with_capacity(tasks.len());
    for task in tasks {
        results.push(
            task.await
                .map_err(|err| EngineError::Message(format!("任务中断：{err}")))?,
        );
    }
    results.sort_by_key(|item| item.index);

    let mut warnings = Vec::new();
    if !parsed.invalid.is_empty() {
        warnings.push(format!("有 {} 行无法识别，已跳过", parsed.invalid.len()));
    }
    if parsed.duplicates > 0 {
        warnings.push(format!("去掉了 {} 条重复链接", parsed.duplicates));
    }

    let mut articles = Vec::new();
    let mut failures = Vec::new();
    let mut summaries = Vec::new();
    let mut cached = 0;
    for item in results {
        for warning in item.warnings {
            if warnings.len() < 40 {
                warnings.push(warning);
            }
        }
        match item.outcome {
            Outcome::Ready {
                article,
                cached: hit,
            } => {
                if hit {
                    cached += 1;
                }
                summaries.push(ArticleSummary {
                    title: article.title.clone(),
                    url: article.source_url.clone(),
                    cached: hit,
                    kind: article.kind.label().to_string(),
                });
                articles.push((item.index + 1, article));
            }
            Outcome::Failed { error, attempts } => {
                failures.push(FailureRecord {
                    url: item.url,
                    error,
                    attempts,
                });
            }
        }
    }

    emit(
        sink.as_ref(),
        &job_id,
        "assemble",
        0,
        total,
        "",
        None,
        "assemble",
        "正在组装 book.md",
        0,
    );
    let draft: BookDraft =
        draft_from_articles(opts.book_title.clone(), opts.book_author.clone(), articles);
    write_book(&draft, &opts.output_dir)?;
    let book_markdown = opts.output_dir.join("book.md");
    let failures_markdown = opts.output_dir.join("failures.md");
    std::fs::write(&failures_markdown, render_failures(&failures))?;

    let book_epub = if opts.export_epub && !draft.chapters.is_empty() {
        emit(
            sink.as_ref(),
            &job_id,
            "epub",
            0,
            total,
            "",
            None,
            "epub",
            "正在生成 EPUB",
            0,
        );
        let path = opts.output_dir.join("book.epub");
        write_epub(&draft, &opts.output_dir, &path)?;
        Some(path)
    } else {
        if opts.export_epub {
            warnings.push("没有成功的文章，未生成 EPUB".into());
        }
        None
    };

    let succeeded = summaries.len();
    let failed = failures.len();
    emit(
        sink.as_ref(),
        &job_id,
        "done",
        0,
        total,
        "",
        None,
        "done",
        &format!("完成：成功 {succeeded}，失败 {failed}"),
        0,
    );
    info!(%job_id, succeeded, failed, cached, "归档结束");

    Ok(JobReport {
        job_id,
        output_dir: path_string(&opts.output_dir),
        book_markdown: path_string(&book_markdown),
        book_epub: book_epub.as_ref().map(|path| path_string(path)),
        failures_markdown: path_string(&failures_markdown),
        succeeded,
        failed,
        cached,
        failures,
        articles: summaries,
        warnings,
    })
}

#[allow(clippy::too_many_arguments)]
async fn process_one(
    job_id: String,
    index: usize,
    total: usize,
    url: String,
    client: Arc<dyn HttpClient>,
    sink: Arc<dyn ProgressSink>,
    cache: Arc<tokio::sync::Mutex<ArticleCache>>,
    pace: Arc<Pace>,
    use_cache: bool,
    retries: u32,
    localize: bool,
    output_dir: PathBuf,
) -> OneResult {
    if use_cache {
        let cached = cache.lock().await.get(&url).ok().flatten();
        if let Some(article) = cached {
            return finish_article(
                article, true, job_id, index, total, url, client, sink, pace, localize, output_dir,
            )
            .await;
        }
    }

    let max_attempts = retries.saturating_add(1).max(1);
    let mut last_error = String::from("抓取失败");
    let mut attempts = 0;
    while attempts < max_attempts {
        attempts += 1;
        let status = if attempts == 1 {
            "fetching"
        } else {
            "retrying"
        };
        let message = if attempts == 1 {
            "正在抓取".to_string()
        } else {
            format!("第 {attempts} 次尝试")
        };
        emit(
            sink.as_ref(),
            &job_id,
            "fetch",
            index,
            total,
            &url,
            None,
            status,
            &message,
            attempts,
        );
        if attempts > 1 {
            tokio::time::sleep(Duration::from_millis(350 * u64::from(attempts))).await;
        }
        pace.wait().await;
        match client.get(&url).await {
            Ok(page) => {
                if !(200..300).contains(&page.status) {
                    last_error = format!("HTTP {}", page.status);
                    if matches!(page.status, 400 | 401 | 403 | 404 | 410 | 451) {
                        break;
                    }
                    continue;
                }
                let html = decode_document(&page.body, &page.content_type);
                let base = if page.final_url.is_empty() {
                    url.clone()
                } else {
                    page.final_url.clone()
                };
                match extract_html(&html, &base) {
                    Ok(mut article) => {
                        article.source_url = url.clone();
                        if article.final_url.is_empty() {
                            article.final_url = base;
                        }
                        if let Err(err) = cache.lock().await.put(&article) {
                            tracing::warn!("写入缓存失败：{err}");
                        }
                        return finish_article(
                            article, false, job_id, index, total, url, client, sink, pace,
                            localize, output_dir,
                        )
                        .await;
                    }
                    Err(err) => {
                        last_error = err.to_string();
                        break;
                    }
                }
            }
            Err(err) => {
                last_error = err.to_string();
            }
        }
    }

    emit(
        sink.as_ref(),
        &job_id,
        "fetch",
        index,
        total,
        &url,
        None,
        "failed",
        &format!("失败：{last_error}"),
        attempts,
    );
    OneResult {
        index,
        url,
        outcome: Outcome::Failed {
            error: last_error,
            attempts,
        },
        warnings: Vec::new(),
    }
}

async fn finish_article(
    mut article: Article,
    cached: bool,
    job_id: String,
    index: usize,
    total: usize,
    url: String,
    client: Arc<dyn HttpClient>,
    sink: Arc<dyn ProgressSink>,
    pace: Arc<Pace>,
    localize: bool,
    output_dir: PathBuf,
) -> OneResult {
    let mut warnings = Vec::new();
    if localize {
        emit(
            sink.as_ref(),
            &job_id,
            "images",
            index,
            total,
            &url,
            Some(article.title.clone()),
            "images",
            "正在下载图片",
            1,
        );
        warnings.extend(
            localize_images(&mut article, client.as_ref(), &output_dir, pace.as_ref()).await,
        );
    }
    let status = if cached { "cached" } else { "ok" };
    let message = if cached {
        "已使用缓存"
    } else {
        "已完成"
    };
    emit(
        sink.as_ref(),
        &job_id,
        "fetch",
        index,
        total,
        &url,
        Some(article.title.clone()),
        status,
        message,
        1,
    );
    OneResult {
        index,
        url,
        outcome: Outcome::Ready { article, cached },
        warnings,
    }
}

fn emit(
    sink: &dyn ProgressSink,
    job_id: &str,
    phase: &str,
    index: usize,
    total: usize,
    url: &str,
    title: Option<String>,
    status: &str,
    message: &str,
    attempt: u32,
) {
    sink.emit(ProgressEvent {
        job_id: job_id.to_string(),
        phase: phase.to_string(),
        index,
        total,
        url: url.to_string(),
        title,
        status: status.to_string(),
        message: message.to_string(),
        attempt,
    });
}

fn path_string(path: &std::path::Path) -> String {
    path.to_string_lossy().to_string()
}

fn next_job_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("job-{millis}-{n}")
}

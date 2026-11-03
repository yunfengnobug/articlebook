# ArticleBook

ArticleBook 是一个本地桌面工具，把你已经拿到的文章链接整理成可离线阅读的 Markdown 和 EPUB。链接列表里可以混用微信文章和普通网页。公众号模式本期只支持手动导入链接文件，真正的登录抓取留到以后。

这是个人阅读归档，不是公网服务。它不绕过付费墙，不伪装微信客户端，也不批量抓取你没有保存权限的公众号。

## 功能

- 顶栏切换两种模式：链接列表、公众号。
- 链接会做规范化：主机小写、去掉片段和常见追踪参数；微信文章保留 `__biz`、`mid`、`idx`、`sn`。
- 微信 `mp.weixin.qq.com` 按页面结构提取；其他网页走通用正文提取。
- 批处理把进度发回界面，按 URL 哈希缓存，失败会重试，单篇失败不会中断其余文章。
- 输出 `book.md`（带目录）、`articles/001.md` 这样的单篇，以及可选的 `book.epub`。
- 可选把图片下载到 `images/`，并改写 Markdown / EPUB 里的地址。
- 公众号目录通过 `AccountProvider` 接入。v1 只有 `Manual`。WMPF / 浏览器登录只在界面上标明未实现。

## 开发

需要 Node.js 22、Rust stable，以及桌面版对应的系统 WebView（Windows 上是 WebView2）。

```bash
npm install
cargo test -p articlebook-engine
npm run tauri dev
```

只看界面、不跑桌面壳：

```bash
npm run dev
```

开发服务器是 [http://127.0.0.1:43123](http://127.0.0.1:43123)。浏览器里可以切换模式；抓取和写文件要在 Tauri 窗口里进行。

离线把仓库里的 HTML 样例收成一本书：

```bash
cargo run -p articlebook-engine --bin articlebook-cli -- assemble-html \
  --out ./tmp-book --title 样例 \
  fixtures/wechat_sample.html fixtures/generic_sample.html
```

从链接文件实抓（只抓你有权保存的 URL）：

```bash
cargo run -p articlebook-engine --bin articlebook-cli -- batch \
  --input fixtures/sample-links.txt --out ./tmp-book --title 样例 --no-epub
```

## 测试

`cargo test -p articlebook-engine` 不访问网络。覆盖：

- URL 规范化与去重
- `fixtures/wechat_sample.html`、`fixtures/generic_sample.html` 的正文提取
- 整书目录顺序、`book.md` 和 EPUB
- 批处理的失败跳过、重试和缓存
- 图片本地化
- Manual provider，以及 WMPF provider 会明确拒绝

## 数据目录

命令行用 `directories` 约定：

| 用途 | Linux | Windows |
| --- | --- | --- |
| 数据 | `~/.local/share/articlebook` | `%APPDATA%\ArticleBook\ArticleBook\data` |
| 缓存（SQLite） | `~/.cache/articlebook/articles.db` | `%LOCALAPPDATA%\ArticleBook\ArticleBook\cache\articles.db` |
| 日志 | `~/.local/share/articlebook/logs/articlebook.log` | 数据目录下的 `logs\articlebook.log` |

桌面版改用 Tauri 的应用数据、缓存和日志目录，缓存文件仍是 `articles.db`。界面底部会显示日志和缓存路径。

默认请求间隔 600ms，并发 2。User-Agent 是带 `ArticleBook/0.1` 的桌面浏览器标识，便于站点返回普通 HTML。它不是微信客户端令牌，也不携带登录态。遇到「环境异常 / 去验证」会记为失败，而不是想办法绕过。

## Windows 打包

本机需要 Visual Studio 的 C++ 生成工具、WebView2、Rust 和 Node。在仓库根目录：

```bash
npm install
npm run tauri build -- --bundles nsis
```

安装包在 `src-tauri/target/release/bundle/nsis/`。

GitHub Actions 只在 `windows-latest` 上构建，触发方式是打 `v*` 标签或手动 `workflow_dispatch`。工作流文件是 `.github/workflows/build-windows.yml`。标签构建会额外上传一个草稿 Release；手动运行只上传构建产物。没有 macOS 或 Linux 的发布任务。

正文提取是纯 Rust，安装包不附带 Python。如果以后某种页面必须用 Python 解析，可以把解释器放到 `src-tauri/binaries/<name>-<target-triple>`，在 `tauri.conf.json` 的 `bundle.externalBin` 里声明，由 Tauri 打进 Windows 包，再由命令启动。当前没有这条路径。

## 布局

```
src/                          Vue 3 + Vite 界面
src-tauri/                    Tauri 2 命令，转调引擎
crates/articlebook-engine/    抓取、提取、组书、EPUB、缓存
fixtures/                     离线 HTML 样例
.github/workflows/            仅 Windows 的安装包构建
```

## 还没做

- 真实公众号历史列表（浏览器登录或 WMPF）
- PDF
- macOS / Linux 安装包

这些不在本期范围内。

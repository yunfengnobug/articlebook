# ArticleBook

ArticleBook 是一个本地桌面工具，把你已经拿到的文章链接整理成可离线阅读的 Markdown 和 EPUB。链接列表里可以混用微信文章和普通网页。公众号模式可以用你自己登录后的微信网页拉取一个你有权阅读的历史目录，也可以继续手动导入链接。

这是个人阅读归档，不是公网服务。它不绕过付费墙，不伪装微信客户端，也不批量抓取你没有保存权限的公众号。

## 功能

- 顶栏切换两种模式：链接列表、公众号。
- 链接会做规范化：主机小写、去掉片段和常见追踪参数；微信文章保留 `__biz`、`mid`、`idx`、`sn`。
- 微信 `mp.weixin.qq.com` 按页面结构提取；其他网页走通用正文提取。
- 批处理把进度发回界面，按 URL 哈希缓存，失败会重试，单篇失败不会中断其余文章。
- 输出 `book.md`（带目录）、`articles/001.md` 这样的单篇，以及可选的 `book.epub`。
- 可选把图片下载到 `images/`，并改写 Markdown / EPUB 里的地址。
- 公众号目录通过 `AccountProvider` 接入。可选「微信网页会话」或「手动链接文件」。WMPF / Frida 仍标明未实现，程序不会调用它们。

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

离线查看历史目录样例解析出的链接（不登录、不访问网络）：

```bash
cargo run -p articlebook-engine --bin articlebook-cli -- history-parse \
  fixtures/wechat_history_getmsg.json
```

## 测试

`cargo test -p articlebook-engine` 不访问网络。覆盖：

- URL 规范化与去重
- `fixtures/wechat_sample.html`、`fixtures/generic_sample.html` 的正文提取
- 整书目录顺序、`book.md` 和 EPUB
- 批处理的失败跳过、重试和缓存
- 图片本地化
- Manual provider，以及 WMPF provider 会明确拒绝
- 微信网页会话：历史消息 / 公众平台图文的离线样例能解析成链接，再走现有导出；未保存会话会拒绝；请求固定使用带 `ArticleBook/0.1` 的浏览器标识，不会变成微信客户端

## 数据目录

命令行用 `directories` 约定：

| 用途 | Linux | Windows |
| --- | --- | --- |
| 数据 | `~/.local/share/articlebook` | `%APPDATA%\ArticleBook\ArticleBook\data` |
| 缓存（SQLite） | `~/.cache/articlebook/articles.db` | `%LOCALAPPDATA%\ArticleBook\ArticleBook\cache\articles.db` |
| 日志 | `~/.local/share/articlebook/logs/articlebook.log` | 数据目录下的 `logs\articlebook.log` |

桌面版改用 Tauri 的应用数据、缓存和日志目录，缓存文件仍是 `articles.db`。界面底部会显示日志和缓存路径。

默认请求间隔 600ms，并发 2。正文抓取的 User-Agent 是带 `ArticleBook/0.1` 的桌面浏览器标识，便于站点返回普通 HTML。它不是微信客户端令牌。历史目录请求也使用这同一个标识，并另外带上你保存的 Cookie。遇到「环境异常 / 去验证」会失败并停下来，不会绕过。

## 微信网页会话

公众号模式里选「微信网页会话」。它只拉取你这次登录有权阅读的一个账号，然后把得到的文章 URL 交给原来的批处理（进度事件、缓存、`book.md`、`articles/*.md`、可选 `book.epub`）。

两种官方页面可以当目录：

- 读者打开的公众号「历史消息」（地址里有 `__biz`，接口是 `mp/profile_ext?action=getmsg`）。
- 你自己管理的公众号，在公众平台里打开的图文素材列表（地址里有 `token`，接口是 `cgi-bin/appmsg?action=list_ex`）。

登录由你自己完成，有两种入口，都只打开 `https://mp.weixin.qq.com/`：

1. **在应用内打开登录页。** 桌面窗口会再开一个页面。扫码、点验证都在这个页面里由你完成。地址变成历史消息或图文列表后，点「保存会话」。保存时会读取这个窗口里 `mp.weixin.qq.com` 的 Cookie（含 HttpOnly），并记住地址栏里的 `__biz`、`uin`、`key`、`pass_ticket` 或 `token`。这个窗口没有 Tauri 命令权限，页面不能反过来调用 ArticleBook。
2. **用系统浏览器打开。** 浏览器里的 Cookie 不会自动回到应用。把历史页地址粘贴到「历史页地址」。如果预览目录失败，再从开发者工具复制该请求的 Cookie 请求头，粘贴到「Cookie」里。只粘贴一行，不要换行。

会话写在 Tauri 应用数据目录的 `wechat-web-session.json`。字段是 `version`、`kind`（`reader-history` 或 `owner-console`）、`accountName`、`biz`、`uin`、`key`、`passTicket`、`token`、`cookie`、`sourceUrl`、`capturedAt`。在 Unix 上文件模式是 `0600`。Windows 上它位于当前用户的应用数据目录，没有额外的 ACL。界面状态只显示有没有 Cookie，不回显内容。再次保存时如果 Cookie 框留空，会沿用文件里的 Cookie；勾选「清除已保存的 Cookie」才会清掉。日志不记录 Cookie、`key` 或 `pass_ticket`。

历史目录每次 10 篇，默认最多 40 篇，可在 1 到 200 之间改。页与页之间默认间隔 800 毫秒。请求主机固定是 `mp.weixin.qq.com`；若被转到别的站点，或返回验证页 / `ret != 0`，这次会失败。

手动链接文件仍然可用。WMPF 那一行保持灰色，选择它不能开始归档，程序里也没有 Frida 或 WMPF 调用。

### 本机还要自己确认的事

这台开发环境没有完成微信扫码，所以端到端的真实登录没有在这里跑通。请在自己的 Windows 桌面上：

1. `npm run tauri dev`
2. 切到公众号，来源选「微信网页会话」
3. 在应用内打开登录页，自己扫码，打开一个你有权阅读的历史消息页
4. 保存会话，预览目录，选输出目录，开始归档
5. 若预览报会话失效或验证页，在微信网页里自己处理后再保存；不要期待程序绕过
6. 若应用内窗口读到的 Cookie 不够，改用系统浏览器并粘贴 Cookie 请求头

`key` 和 `pass_ticket` 会过期。过期后重新打开历史页再保存一次。

## Windows 打包

本机需要 Visual Studio 的 C++ 生成工具、WebView2、Rust 和 Node。在仓库根目录：

```bash
npm install
npm run tauri build -- --bundles nsis
```

安装包在 `src-tauri/target/release/bundle/nsis/`。

GitHub Actions 只在 `windows-latest` 上构建，触发方式是打 `v*` 标签或手动 `workflow_dispatch`。工作流文件是 `.github/workflows/build-windows.yml`。标签构建会额外上传一个草稿 Release；手动运行只上传构建产物。没有 macOS 或 Linux 的发布任务。

正文提取是纯 Rust，安装包不附带 Python。如果以后某种页面必须用 Python 解析，可以把解释器放到 `src-tauri/binaries/<name>-<target-triple>`，在 `tauri.conf.json` 的 `bundle.externalBin` 里声明，由 Tauri 打进 Windows 包，再由命令启动。当前没有这条路径。

## 自动更新（TODO）

当前 **未接入** Tauri Updater。`v*` 标签会构建 NSIS 安装包并生成草稿 Release，但应用不会检查更新。

后续若要接 GitHub Releases 自动更新，大致需要：

1. 生成本地签名密钥：`npm run tauri -- signer generate -w ~/.tauri/articlebook.key`
2. 把 **公钥** 写入 `src-tauri/tauri.conf.json` 的 `plugins.updater.pubkey`；**私钥** 只放进 GitHub Actions secrets（如 `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`），不要提交仓库
3. 依赖：`tauri-plugin-updater`（Rust）与 `@tauri-apps/plugin-updater`（前端）；在 `lib.rs` 里 `.plugin(tauri_plugin_updater::Builder::new().build())`
4. 配置 `plugins.updater.endpoints` 指向本仓库的 latest.json，例如 `https://github.com/yunfengnobug/articlebook/releases/latest/download/latest.json`（需在发布流水线生成并随 Release 上传 `.sig` 与 `latest.json`）
5. 扩展 `.github/workflows/build-windows.yml`：构建后用私钥签名安装包，上传 `*.exe`、`*.sig` 与 updater 清单；正式发布后再从草稿 Release 发布

在密钥与 secrets 就绪之前，不要半接入 updater（缺少 pubkey / endpoint 会导致构建或运行失败）。

## 布局

```
src/                          Vue 3 + Vite 界面
src-tauri/                    Tauri 2 命令，转调引擎
crates/articlebook-engine/    抓取、提取、组书、EPUB、缓存
fixtures/                     离线 HTML 样例
.github/workflows/            仅 Windows 的安装包构建
```

## 还没做

- 在没有本机微信登录的环境里自动跑通扫码（会话文件要你自己在桌面应用里生成）
- 把 WebView 里的登录态伪装成微信客户端；目录请求始终带 ArticleBook 标识，微信若因此拒绝，就换手动链接
- WMPF、Frida，以及注入微信桌面客户端
- 付费墙、批量抓取你没有阅读权限的公众号
- PDF
- macOS / Linux 安装包
- 基于 GitHub Releases 的 Tauri 自动更新（缺签名密钥与 Actions secrets；见上文「自动更新」）

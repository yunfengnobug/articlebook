# ArticleBook

ArticleBook 是一个本地桌面工具，把你已经拿到的文章链接整理成可离线阅读的 Markdown 和 EPUB。链接列表里可以混用微信文章和普通网页。公众号模式用你自己登录后的微信网页，把一个你有权阅读的号的历史收成一本书；也可以改用手动粘贴链接。

这是个人阅读归档，不是公网服务。它不绕过付费墙，不伪装微信客户端。公众号模式只归档你登录后有权阅读的那一个号，默认拉取它的全部历史（有安全上限）。

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
- 微信网页会话：历史消息 / 公众平台图文的离线样例能解析成链接，再走现有导出；未保存会话会拒绝；`history_limit == 0` 会一直翻到没有下一页，并能超过原先的 200 篇，同时停在 5000 篇 / 1000 页的安全上限；请求固定使用带 `ArticleBook/0.1` 的浏览器标识，不会变成微信客户端

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

公众号模式按三步走，只处理你这次登录后有权阅读的那一个号。拉到的文章 URL 交给原来的批处理（进度事件、缓存、`book.md`、`articles/*.md`、可选 `book.epub`）。

1. **登录微信网页。** 点「在应用内打开登录页」，用微信扫码。需要时再用「用系统浏览器（高级）」。两种入口都只打开 `https://mp.weixin.qq.com/`。
2. **打开要归档的公众号历史。** 在登录后的窗口里打开该号的「历史消息」（地址栏通常带 `__biz`）。你自己管理的号也可以打开公众平台的图文列表（地址里有 `token`）。应用内窗口打开到正确页后，点「我已打开历史消息 · 保存」。
3. **拉取全部文章并归档。** 默认是「全部文章」。选好输出目录，点「开始归档」。想先确认篇数，点「先看看有多少篇」。

两种官方页面可以当目录：

- 读者打开的公众号「历史消息」（地址里有 `__biz`，接口是 `mp/profile_ext?action=getmsg`，看 `can_msg_continue`）。
- 你自己管理的公众号，在公众平台里打开的图文素材列表（地址里有 `token`，接口是 `cgi-bin/appmsg?action=list_ex`）。

「全部文章」会按历史接口一直翻页，直到 `can_msg_continue` / `can_continue` 为假，或这一页已经没有文章。单次还有安全上限：最多 5000 篇、最多 1000 次翻页，避免接口一直说还有下一页时停不下来。传给引擎的 `history_limit == 0` 就是这条「全部」路径；「仅最近 N 篇」则传入 N（同样不超过 5000）。页与页之间间隔 800 毫秒。五千篇的目录大约要几分钟翻完，然后再抓正文。

应用内登录窗口会在保存时读取 `mp.weixin.qq.com` 的 Cookie（含 HttpOnly），并记住地址栏里的 `__biz`、`uin`、`key`、`pass_ticket` 或 `token`。窗口里的地址会自动填回「历史页地址」。这个窗口没有 Tauri 命令权限，页面不能反过来调用 ArticleBook。

系统浏览器里的 Cookie 不会自动回到应用。把历史页地址和请求头 Cookie 贴到折叠的「高级：手动 Cookie」。只粘贴一行，不要换行。留空再保存会沿用文件里的 Cookie；勾选「清除已保存的 Cookie」才会清掉。

会话写在 Tauri 应用数据目录的 `wechat-web-session.json`。字段是 `version`、`kind`（`reader-history` 或 `owner-console`）、`accountName`、`biz`、`uin`、`key`、`passTicket`、`token`、`cookie`、`sourceUrl`、`capturedAt`。在 Unix 上文件模式是 `0600`。Windows 上它位于当前用户的应用数据目录，没有额外的 ACL。界面只显示「已保存某某号」或「未登录」，不回显 Cookie，也不把会话文件路径放在主界面上。日志不记录 Cookie、`key` 或 `pass_ticket`。

历史目录每次向微信要 10 篇。请求主机固定是 `mp.weixin.qq.com`；若被转到别的站点，或返回验证页 / `ret != 0`，这次会失败。

手动链接在「改用手动粘贴链接」里。WMPF 不出现在主路径上，程序里也没有 Frida 或 WMPF 调用。

### 本机还要自己确认的事

这台开发环境没有完成微信扫码，所以端到端的真实登录没有在这里跑通。请在自己的 Windows 桌面上：

1. `npm run tauri dev`
2. 切到公众号
3. 点「在应用内打开登录页」，自己扫码，打开一个你有权阅读的历史消息页
4. 点「我已打开历史消息 · 保存」，确认步骤 3 显示「已保存某某号」
5. 输出目录保持默认或另选，直接「开始归档」（默认拉全部历史，不超过安全上限）
6. 若拉取报会话失效或验证页，在微信网页里自己处理后再保存；不要期待程序绕过
7. 若应用内窗口读到的 Cookie 不够，改用系统浏览器，把地址和 Cookie 贴到「高级：手动 Cookie」

`key` 和 `pass_ticket` 会过期。过期后重新打开历史页再保存一次。

## Windows 打包

本机需要 Visual Studio 的 C++ 生成工具、WebView2、Rust 和 Node。`createUpdaterArtifacts` 打开之后，打包必须能读到私钥，否则 Tauri 会在签名步骤失败。不要设置 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`，这把密钥没有密码。

Windows PowerShell：

```powershell
npm install
$env:TAURI_SIGNING_PRIVATE_KEY = (Resolve-Path .\signing\articlebook.key)
npm run tauri build -- --bundles nsis
```

macOS / Linux：

```bash
npm install
TAURI_SIGNING_PRIVATE_KEY="$PWD/signing/articlebook.key" npm run tauri build -- --bundles nsis
```

这个仓库是 Cargo workspace，安装包在仓库根目录的 `target/release/bundle/nsis/`，不在 `src-tauri/target/`。签名成功时，旁边还有同名 `.sig`。

GitHub Actions 只在 `windows-latest` 上构建，触发方式是打 `v*` 标签或手动 `workflow_dispatch`。工作流文件是 `.github/workflows/build-windows.yml`。标签构建会发布一个正式 Release；手动运行只上传构建产物。没有 macOS 或 Linux 的发布任务。

正文提取是纯 Rust，安装包不附带 Python。如果以后某种页面必须用 Python 解析，可以把解释器放到 `src-tauri/binaries/<name>-<target-triple>`，在 `tauri.conf.json` 的 `bundle.externalBin` 里声明，由 Tauri 打进 Windows 包，再由命令启动。当前没有这条路径。

## 自动更新

Windows NSIS 安装包由 Tauri 2 updater 签名。公钥写在 `src-tauri/tauri.conf.json` 的 `plugins.updater.pubkey`，客户端用它校验下载的安装包。页脚有「检查更新」。桌面窗口启动时也会静默查一次；有新版本时再点「下载并安装」。Windows 上安装程序启动后应用会退出。

更新清单地址是 `https://github.com/yunfengnobug/articlebook/releases/latest/download/latest.json`。

签名密钥在 `signing/`，说明见 `signing/README.md`：

- `signing/articlebook.key`：私钥，故意放在这个私有仓库里，密码为空
- `signing/articlebook.key.pub`：公钥
- GitHub Actions secret `TAURI_SIGNING_PRIVATE_KEY` 是同一把私钥的备份

`.github/workflows/build-windows.yml` 打包时优先把 `TAURI_SIGNING_PRIVATE_KEY` 指到 `signing/articlebook.key`，文件不存在才用 secret。工作流不设置 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。Actions 环境里 `CI=true`，Tauri 会把缺省密码当成空字符串。

`bundle.createUpdaterArtifacts` 为 `true` 时，Tauri 2 直接给 NSIS 安装包签名，产物是 `*-setup.exe` 和 `*-setup.exe.sig`，不是 v1 的 `.nsis.zip`。`v*` 标签构建还会在同一目录写 `latest.json`：`version` 用应用版本，`windows-x86_64` 和 `windows-x86_64-nsis` 都指向这个安装包，`signature` 是 `.sig` 文件的内容。正式 Release 会带上安装包、`.sig` 和 `latest.json`。发布之后，`releases/latest` 会把这份清单交给已安装的客户端。应用版本必须高于正在运行的版本，客户端才会安装；当前版本是 `0.1.2`。

仓库仍是私有时，没有登录的客户端访问 GitHub Release 下载地址会得到 404。自动更新要等仓库公开，或改成客户端能够匿名下载的地址。

## 布局

```
src/                          Vue 3 + Vite 界面
src-tauri/                    Tauri 2 命令，转调引擎
crates/articlebook-engine/    抓取、提取、组书、EPUB、缓存
fixtures/                     离线 HTML 样例
.github/workflows/            仅 Windows 的安装包构建
signing/                      updater 签名密钥（私钥故意留在本私有仓库）
```

## 还没做

- 在没有本机微信登录的环境里自动跑通扫码（会话文件要你自己在桌面应用里生成）
- 把 WebView 里的登录态伪装成微信客户端；目录请求始终带 ArticleBook 标识，微信若因此拒绝，就换手动链接
- WMPF、Frida，以及注入微信桌面客户端
- 付费墙，以及你登录后仍打不开的公众号
- PDF
- macOS / Linux 安装包

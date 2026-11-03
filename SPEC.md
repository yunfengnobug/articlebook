# ArticleBook — 规格（已确认）

形态：Tauri 桌面应用（UI + 本地引擎）；新建私有仓库。
CI：GitHub Actions **仅构建 Windows** 安装包/便携包（不打 macOS/Linux）。
用途：个人阅读归档（链接列表 / 指定公众号；通用网页 + 微信文章）。

## 非目标
- 不做公网 SaaS / 多用户
- 不绕过付费墙、不做未授权商用批量扒号
- 互动组件最多占位
- 首期不做 macOS/Linux 发布产物

## 模式
1. **链接列表**：多行 URL / 文件 / 粘贴抽链；可混微信与普通网页
2. **公众号**：输入标识 → Provider 拉历史列表 → 同一正文管道；首期 Provider = Manual（导入链接文件），接口预留；登录浏览器/WMPF 后续

UI 顶栏模式切换按钮。

## P0
- 模式切换 UI（Tauri 前端）
- URL 解析去重（可配置剥离追踪参数）
- 通用正文提取 + 微信 mp.weixin.qq.com 适配
- 图片可选本地化
- 导出：单篇 + 整书 Markdown（TOC）；EPUB
- 进度、失败重试、跳过继续
- 本地缓存、限速/并发、日志与失败清单
- GitHub Actions：push tag / workflow_dispatch → Windows artifact only

## 技术建议
- 后端/引擎：Rust（Tauri commands）或 sidecar Python；优先 **Rust 引擎 + Tauri 2**，若正文库选型困难可用 Python sidecar 由 Tauri 拉起
- 前端：Vite + React 或 Svelte（任选其一，保持简单）
- 存储：应用数据目录 + SQLite
- EPUB：Rust crate 或 sidecar 生成

## 包结构建议
```
articlebook/
  README.md SPEC.md
  src-tauri/          # Tauri + 引擎 commands
  src/                # 前端
  .github/workflows/build-windows.yml  # windows-only
  fixtures/ tests/
```

## 本任务实施顺序（1–7）
1. 脚手架：Tauri 2 项目、配置、日志、数据目录约定、基础 CLI/command
2. 单 URL → Markdown（微信 fixture + 普通网页 fixture/实抓）
3. 链接列表批处理 + 进度事件 + 缓存 + 重试
4. 整书组装 + TOC + 图片本地化开关
5. EPUB 导出
6. UI：模式切换；列表模式接线可跑任务并选输出目录
7. 公众号 Provider 协议 + Manual 保底
另：加 Windows-only GitHub Actions（release 产物）

步骤 8+（真实公众号目录、WMPF、PDF、非 Windows CI）不做，README 写后续。

## 验收
- README：开发启动、Windows 打包说明
- Actions 仅 `windows-latest` job
- 列表模式可导出 book.md / epub
- 模式切换可见；公众号模式可用手动链接文件

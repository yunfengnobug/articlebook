<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import ModeSwitch from "./components/ModeSwitch.vue";
import ProgressCard from "./components/ProgressCard.vue";
import {
  chooseDirectory,
  chooseLinksFile,
  clearWechatSession,
  describeWechatPage,
  errorText,
  readTextFile,
  inTauri,
  listenProgress,
  listenWechatPage,
  loadPaths,
  loadProviders,
  loadSuggestedDir,
  loadWechatSession,
  openOutput,
  openWechatLoginSite,
  checkForUpdate,
  installAppUpdate,
  openWechatLoginWindow,
  previewLinks,
  previewWechatHistory,
  runAccountExport,
  runListExport,
  saveWechatSession,
} from "./desktop";
import type {
  AppPaths,
  ExportSettings,
  JobReport,
  Mode,
  ProgressEvent,
  ProgressRow,
  ProviderInfo,
  SessionSummary,
  WechatPageEvent,
} from "./types";

const fallbackProviders: ProviderInfo[] = [
  {
    id: "wechat-web",
    name: "微信网页会话",
    available: true,
    note: "用你自己登录后的微信网页，拉取一个你有权阅读的公众号的全部历史。单次有安全上限。不会调用 Frida 或 WMPF，也不会伪装微信客户端。",
  },
  {
    id: "manual",
    name: "手动链接文件",
    available: true,
    note: "导入你已经复制好的文章链接。不会登录微信。",
  },
  {
    id: "wmpf",
    name: "微信登录 / WMPF",
    available: false,
    note: "尚未实现。不会登录微信，也不会调用 Frida 或 WMPF。",
  },
];

const desktop = ref(inTauri());
const mode = ref<Mode>("list");
const urlsText = ref("");
const accountName = ref("");
const accountId = ref("");
const linksText = ref("");
const linksPath = ref("");
const providerId = ref("wechat-web");
const providers = ref<ProviderInfo[]>(fallbackProviders);
const pageUrl = ref("");
const cookieText = ref("");
const clearCookie = ref(false);
/** Keep in sync with `SAFETY_MAX_ARTICLES` in the engine. `0` means the whole history. */
const SAFETY_MAX_ARTICLES = 5000;
const fetchAll = ref(true);
const historyLimit = ref(40);
const session = ref<SessionSummary | null>(null);
const pageHint = ref("");
const bookTitle = ref("");
const bookAuthor = ref("");
const outputDir = ref("");
const localizeImages = ref(false);
const exportEpub = ref(true);
const stripTracking = ref(true);
const useCache = ref(true);
const concurrency = ref(2);
const retries = ref(2);
const delayMs = ref(600);
const running = ref(false);
const error = ref("");
const previewText = ref("");
const phase = ref("");
const rows = ref<ProgressRow[]>([]);
const report = ref<JobReport | null>(null);
const paths = ref<AppPaths | null>(null);
const updateBusy = ref(false);
const updateAvailable = ref(false);
const updateMessage = ref("");
const fileInput = ref<HTMLInputElement | null>(null);
let unlistenProgress: (() => void) | null = null;
let unlistenPage: (() => void) | null = null;

const selectedProvider = computed(
  () => providers.value.find((item) => item.id === providerId.value) ?? providers.value[0],
);
const providerBlocked = computed(() => mode.value === "account" && selectedProvider.value?.available === false);
const wechatMode = computed(() => mode.value === "account" && providerId.value === "wechat-web");
const manualMode = computed(() => mode.value === "account" && providerId.value === "manual");
const runDisabled = computed(() => running.value || providerBlocked.value);
const sessionLine = computed(() => describeSession(session.value));

function settings(): ExportSettings {
  return {
    outputDir: outputDir.value.trim(),
    bookTitle: bookTitle.value.trim(),
    bookAuthor: bookAuthor.value.trim(),
    localizeImages: localizeImages.value,
    exportEpub: exportEpub.value,
    stripTracking: stripTracking.value,
    concurrency: Number(concurrency.value) || 2,
    retries: Number(retries.value) || 0,
    delayMs: Number(delayMs.value) || 0,
    useCache: useCache.value,
  };
}

function historyLimitForRequest(): number {
  if (fetchAll.value) return 0;
  const value = Number(historyLimit.value);
  if (!Number.isFinite(value) || value < 1) return 40;
  return Math.min(SAFETY_MAX_ARTICLES, Math.max(1, Math.round(value)));
}

function chooseRecent() {
  fetchAll.value = false;
}

function describeSession(value: SessionSummary | null): string {
  if (!value?.saved) return "未登录";
  const name = value.accountName.trim() || "这个公众号";
  return `已保存「${name}」`;
}

function onProgress(event: ProgressEvent) {
  if (!event.url) {
    phase.value = event.message;
    return;
  }
  const row: ProgressRow = {
    index: event.index,
    total: event.total,
    url: event.url,
    title: event.title,
    status: event.status,
    message: event.message,
    attempt: event.attempt,
  };
  const existing = rows.value.findIndex((item) => item.index === event.index);
  if (existing >= 0) rows.value[existing] = row;
  else rows.value.push(row);
  rows.value.sort((a, b) => a.index - b.index);
}

function onWechatPage(event: WechatPageEvent) {
  pageHint.value = event.hint.message;
  if (event.hint.usable) pageUrl.value = event.url;
}

function requireDesktop(action: string): boolean {
  if (desktop.value) return true;
  error.value = `浏览器预览不能${action}。请在 ArticleBook 桌面窗口里操作。`;
  return false;
}

async function preview() {
  error.value = "";
  if (wechatMode.value) {
    if (!requireDesktop("查看篇数")) return;
    if (!session.value?.saved) {
      error.value = "还没有登录。请先完成步骤 1 和步骤 2。";
      return;
    }
    try {
      const previewed = await previewWechatHistory(accountName.value, historyLimitForRequest());
      const shown = previewed.urls.slice(0, 6);
      const rest = previewed.count - shown.length;
      const lines = [`共 ${previewed.count} 篇。`];
      if (shown.length) lines.push(shown.join("\n"));
      if (rest > 0) lines.push(`还有 ${rest} 篇，开始归档时会一并保存。`);
      if (previewed.sourceNote.includes("安全上限")) {
        lines.push("已到单次安全上限，更早的文章这次不会继续拉。");
      }
      previewText.value = lines.join("\n");
      if (!accountName.value && previewed.accountName) accountName.value = previewed.accountName;
    } catch (err) {
      error.value = errorText(err);
    }
    return;
  }
  const text = mode.value === "list" ? urlsText.value : linksText.value;
  if (!text.trim() && !linksPath.value) {
    previewText.value = "还没有链接。";
    return;
  }
  try {
    const parsed = await previewLinks(text, stripTracking.value);
    const rough = desktop.value ? "" : "（浏览器里是粗略计数，桌面应用会做正式去重）";
    previewText.value = `识别到 ${parsed.urls.length} 条链接，重复 ${parsed.duplicates} 条，无法识别 ${parsed.invalid.length} 行。${rough}`;
  } catch (err) {
    error.value = errorText(err);
  }
}

async function browseOutput() {
  if (!desktop.value) {
    error.value = "浏览器预览不能打开系统文件夹选择器。请在桌面窗口里选择，或直接填写路径。";
    return;
  }
  try {
    const selected = await chooseDirectory();
    if (selected) outputDir.value = selected;
  } catch (err) {
    error.value = errorText(err);
  }
}

async function browseLinks() {
  if (!desktop.value) {
    fileInput.value?.click();
    return;
  }
  try {
    const selected = await chooseLinksFile();
    if (!selected) return;
    if (mode.value === "list") {
      const text = await readTextFile(selected);
      urlsText.value = urlsText.value.trim() ? `${urlsText.value.trim()}\n${text}` : text;
    } else {
      linksPath.value = selected;
    }
  } catch (err) {
    error.value = errorText(err);
  }
}

function onLocalFile(event: Event) {
  const input = event.target as HTMLInputElement;
  const file = input.files?.[0];
  input.value = "";
  if (!file) return;
  const reader = new FileReader();
  reader.onload = () => {
    const text = String(reader.result ?? "");
    if (mode.value === "list") {
      urlsText.value = urlsText.value.trim() ? `${urlsText.value.trim()}\n${text}` : text;
    } else {
      linksText.value = text;
      linksPath.value = "";
    }
  };
  reader.readAsText(file);
}

async function openLoginWindow() {
  error.value = "";
  if (!requireDesktop("打开登录窗口")) return;
  try {
    await openWechatLoginWindow();
    pageHint.value = "登录窗口已打开。用微信扫码，然后打开该公众号的「历史消息」。";
  } catch (err) {
    error.value = errorText(err);
  }
}

async function openLoginSite() {
  error.value = "";
  if (!requireDesktop("打开系统浏览器")) return;
  try {
    await openWechatLoginSite();
    pageHint.value =
      "已打开系统浏览器。登录后把历史页地址贴到「高级：手动 Cookie」。浏览器里的 Cookie 不会自动回到应用。";
  } catch (err) {
    error.value = errorText(err);
  }
}

async function describePage() {
  if (!desktop.value || !pageUrl.value.trim()) return;
  try {
    const hint = await describeWechatPage(pageUrl.value);
    pageHint.value = hint.message;
  } catch (err) {
    error.value = errorText(err);
  }
}

async function saveSession() {
  error.value = "";
  if (!requireDesktop("保存会话")) return;
  try {
    const saved = await saveWechatSession({
      pageUrl: pageUrl.value,
      cookie: cookieText.value,
      accountName: accountName.value,
      clearCookie: clearCookie.value,
    });
    session.value = saved;
    cookieText.value = "";
    clearCookie.value = false;
    if (!accountName.value && saved.accountName) accountName.value = saved.accountName;
    if (saved.sourceUrl) pageUrl.value = saved.sourceUrl;
    pageHint.value = saved.hasCookie
      ? "已保存。回到步骤 3，点「开始归档」。"
      : "地址已保存，但没有 Cookie。打开「高级：手动 Cookie」，粘贴请求头里的一行后再保存一次。";
  } catch (err) {
    error.value = errorText(err);
  }
}

async function clearSession() {
  error.value = "";
  if (!requireDesktop("清除会话")) return;
  try {
    session.value = await clearWechatSession();
    cookieText.value = "";
    clearCookie.value = false;
    pageHint.value = "已清除本机登录。";
  } catch (err) {
    error.value = errorText(err);
  }
}

async function run() {
  error.value = "";
  previewText.value = "";
  if (!desktop.value) {
    error.value = "请在 ArticleBook 桌面窗口中运行导出。浏览器预览只能切换模式和查看界面。";
    return;
  }
  if (!outputDir.value.trim()) {
    error.value = "请先选择输出目录。";
    return;
  }
  if (mode.value === "list" && !urlsText.value.trim()) {
    error.value = "请粘贴至少一条链接。";
    return;
  }
  if (manualMode.value && !linksText.value.trim() && !linksPath.value.trim()) {
    error.value = "请粘贴链接，或选择链接文件。";
    return;
  }
  if (wechatMode.value && !session.value?.saved) {
    error.value = "还没有登录。请先完成步骤 1 和步骤 2。";
    return;
  }
  running.value = true;
  rows.value = [];
  report.value = null;
  phase.value = "准备归档";
  try {
    const body = settings();
    report.value =
      mode.value === "list"
        ? await runListExport({ urlsText: urlsText.value, ...body })
        : await runAccountExport({
            providerId: providerId.value,
            accountName: accountName.value,
            accountId: accountId.value,
            linksText: linksText.value,
            linksPath: linksPath.value || null,
            historyLimit: historyLimitForRequest(),
            ...body,
          });
    phase.value = `完成：成功 ${report.value.succeeded}，失败 ${report.value.failed}`;
  } catch (err) {
    error.value = errorText(err);
  } finally {
    running.value = false;
  }
}

async function checkUpdates(interactive: boolean) {
  if (!desktop.value) {
    if (interactive) updateMessage.value = "检查更新需要在桌面窗口里运行。";
    return;
  }
  updateBusy.value = true;
  try {
    const status = await checkForUpdate();
    updateAvailable.value = status.available;
    if (interactive || status.available) updateMessage.value = status.message;
  } catch (err) {
    updateAvailable.value = false;
    if (interactive) updateMessage.value = errorText(err);
  } finally {
    updateBusy.value = false;
  }
}

async function onInstallUpdate() {
  if (!desktop.value) return;
  updateBusy.value = true;
  updateMessage.value = "正在下载更新。Windows 上安装开始后应用会退出。";
  try {
    await installAppUpdate();
    updateMessage.value = "更新已安装。";
  } catch (err) {
    updateMessage.value = errorText(err);
  } finally {
    updateBusy.value = false;
  }
}

async function openDir() {
  if (!report.value) return;
  try {
    await openOutput(report.value.outputDir);
  } catch (err) {
    error.value = errorText(err);
  }
}

onMounted(async () => {
  if (!desktop.value) return;
  try {
    unlistenProgress = await listenProgress(onProgress);
    unlistenPage = await listenWechatPage(onWechatPage);
    paths.value = await loadPaths();
    const suggested = await loadSuggestedDir();
    if (suggested && !outputDir.value) outputDir.value = suggested;
    const remote = await loadProviders();
    if (remote && remote.length > 0) providers.value = remote;
    session.value = await loadWechatSession();
    if (session.value.saved) {
      if (!accountName.value && session.value.accountName) accountName.value = session.value.accountName;
      if (session.value.sourceUrl) pageUrl.value = session.value.sourceUrl;
    }
  } catch (err) {
    error.value = errorText(err);
  }
  await checkUpdates(false);
});

onUnmounted(() => {
  unlistenProgress?.();
  unlistenPage?.();
});
</script>

<template>
  <div class="shell">
    <header class="top">
      <div class="brand">
        <span class="mark" aria-hidden="true">册</span>
        <div>
          <p class="eyebrow">个人阅读归档</p>
          <h1>ArticleBook</h1>
        </div>
      </div>
      <ModeSwitch v-model="mode" />
    </header>

    <p v-if="!desktop" class="banner">
      当前是浏览器预览：可以切换模式，并查看公众号的三个步骤。登录、保存和归档需要在桌面窗口里运行。
    </p>
    <p v-if="error" class="banner bad" role="alert">{{ error }}</p>

    <main>
      <section class="card composer">
        <header class="card-head">
          <h2>{{ mode === "list" ? "链接列表" : "公众号" }}</h2>
          <p v-if="mode === 'list'">微信文章和普通网页可以混在一起。空行和 # 注释会跳过。</p>
          <p v-else-if="wechatMode">登录你自己的微信，把一个你能阅读的公众号历史收成一本书。</p>
          <p v-else-if="manualMode">导入你已经复制好的链接。不会登录微信。</p>
          <p v-else>{{ selectedProvider?.note }}</p>
        </header>

        <template v-if="mode === 'list'">
          <label class="field">
            <span>文章链接</span>
            <textarea
              v-model="urlsText"
              rows="9"
              spellcheck="false"
              :disabled="running"
              placeholder="https://mp.weixin.qq.com/s/……&#10;https://example.com/posts/weekend-reading"
            ></textarea>
          </label>
        </template>

        <div v-else-if="wechatMode" class="steps">
          <section class="step">
            <h3>步骤 1 · 登录微信网页</h3>
            <div class="step-actions">
              <button type="button" class="primary" :disabled="running" @click="openLoginWindow">
                在应用内打开登录页
              </button>
              <button type="button" class="linkish" :disabled="running" @click="openLoginSite">
                用系统浏览器（高级）
              </button>
            </div>
            <p class="tip">用微信扫码登录。</p>
          </section>

          <section class="step">
            <h3>步骤 2 · 打开要归档的公众号历史</h3>
            <p class="tip">
              登录后打开该公众号的「历史消息」页面（地址栏通常带 __biz）。自己管理的号，打开图文列表也行。应用内窗口打开到正确页后，点「我已打开历史消息 · 保存」。
            </p>
            <button type="button" class="primary" :disabled="running" @click="saveSession">
              我已打开历史消息 · 保存
            </button>
            <p v-if="pageHint" class="note">{{ pageHint }}</p>
            <details class="cookie-box">
              <summary>高级：手动 Cookie</summary>
              <p class="note">
                应用内登录会自己带上 Cookie。只有改用系统浏览器、而且拉取失败时，才把历史页地址和请求头里的一行 Cookie 贴在这里。不要换行。
              </p>
              <label class="field">
                <span>历史页地址</span>
                <textarea
                  v-model="pageUrl"
                  rows="3"
                  spellcheck="false"
                  :disabled="running"
                  placeholder="应用内打开到历史消息后会自动填入。系统浏览器请粘贴地址栏。"
                  @change="describePage"
                ></textarea>
              </label>
              <textarea
                v-model="cookieText"
                rows="3"
                spellcheck="false"
                :disabled="running"
                placeholder="留空则沿用已保存的 Cookie。填了新的会替换。"
              ></textarea>
              <label class="checks">
                <input v-model="clearCookie" type="checkbox" :disabled="running" />
                清除已保存的 Cookie
              </label>
            </details>
          </section>

          <section class="step">
            <h3>步骤 3 · 拉取全部文章并归档</h3>
            <p class="session-line">
              {{ sessionLine }}
              <button
                v-if="session?.saved"
                type="button"
                class="linkish"
                :disabled="running"
                @click="clearSession"
              >
                清除登录
              </button>
            </p>
            <div class="choice" role="radiogroup" aria-label="拉取范围">
              <label>
                <input v-model="fetchAll" type="radio" :value="true" :disabled="running" />
                全部文章
              </label>
              <label>
                <input v-model="fetchAll" type="radio" :value="false" :disabled="running" />
                仅最近
                <input
                  v-model.number="historyLimit"
                  class="narrow"
                  :class="{ idle: fetchAll }"
                  type="number"
                  min="1"
                  :max="SAFETY_MAX_ARTICLES"
                  :disabled="running"
                  aria-label="最近篇数"
                  @focus="chooseRecent"
                  @input="chooseRecent"
                />
                篇
              </label>
            </div>
            <p class="tip">选「全部文章」会一直翻到没有更多为止，单次最多 {{ SAFETY_MAX_ARTICLES }} 篇。</p>
            <label class="field">
              <span>输出目录</span>
              <div class="path-row">
                <input v-model="outputDir" type="text" :disabled="running" placeholder="选择保存 book.md 的文件夹" />
                <button type="button" class="ghost" :disabled="running" @click="browseOutput">选择</button>
              </div>
            </label>
            <details>
              <summary>书名和导出选项</summary>
              <div class="split">
                <label class="field">
                  <span>书名</span>
                  <input v-model="bookTitle" type="text" :disabled="running" placeholder="留空则用公众号名称" />
                </label>
                <label class="field">
                  <span>署名</span>
                  <input v-model="bookAuthor" type="text" :disabled="running" placeholder="个人归档" />
                </label>
              </div>
              <div class="checks">
                <label><input v-model="exportEpub" type="checkbox" :disabled="running" /> 导出 EPUB</label>
                <label><input v-model="localizeImages" type="checkbox" :disabled="running" /> 下载图片到本地</label>
                <label><input v-model="stripTracking" type="checkbox" :disabled="running" /> 去掉追踪参数</label>
                <label><input v-model="useCache" type="checkbox" :disabled="running" /> 使用本地缓存</label>
              </div>
              <div class="split three">
                <label class="field">
                  <span>并发</span>
                  <input v-model.number="concurrency" type="number" min="1" max="6" :disabled="running" />
                </label>
                <label class="field">
                  <span>失败重试</span>
                  <input v-model.number="retries" type="number" min="0" max="5" :disabled="running" />
                </label>
                <label class="field">
                  <span>间隔（毫秒）</span>
                  <input v-model.number="delayMs" type="number" min="0" max="10000" step="100" :disabled="running" />
                </label>
              </div>
            </details>
            <button type="button" class="primary block" :disabled="runDisabled" @click="run">
              {{ running ? "正在归档…" : "开始归档" }}
            </button>
            <div class="step-actions">
              <button type="button" class="ghost" :disabled="running" @click="preview">先看看有多少篇</button>
            </div>
            <p v-if="previewText" class="note preview-block">{{ previewText }}</p>
          </section>

          <p class="switch-line">
            <button type="button" class="linkish" :disabled="running" @click="providerId = 'manual'">
              改用手动粘贴链接
            </button>
          </p>
        </div>

        <template v-else-if="manualMode">
          <p class="switch-line">
            <button type="button" class="linkish" :disabled="running" @click="providerId = 'wechat-web'">
              返回微信网页登录
            </button>
          </p>
          <label class="field">
            <span>合集名称</span>
            <input v-model="accountName" type="text" :disabled="running" placeholder="例如：周末读完" />
          </label>
          <label class="field">
            <span>链接</span>
            <textarea
              v-model="linksText"
              rows="7"
              spellcheck="false"
              :disabled="running"
              placeholder="每行一条，或从文件导入"
            ></textarea>
          </label>
          <p v-if="linksPath" class="note">将读取文件：{{ linksPath }}</p>
        </template>
        <p v-else class="note">这个来源不能开始归档。请改用微信网页会话或手动链接。</p>

        <div v-if="mode === 'list' || manualMode" class="row-actions">
          <button type="button" class="ghost" :disabled="running" @click="browseLinks">从文件导入</button>
          <button type="button" class="ghost" :disabled="running" @click="preview">识别链接</button>
          <input ref="fileInput" class="hidden" type="file" accept=".txt,.md,.csv,text/plain" @change="onLocalFile" />
        </div>
        <p v-if="previewText && !wechatMode" class="note preview-block">{{ previewText }}</p>

        <template v-if="!wechatMode">
          <div class="split">
            <label class="field">
              <span>书名</span>
              <input v-model="bookTitle" type="text" :disabled="running" placeholder="留空则用今天的日期" />
            </label>
            <label class="field">
              <span>署名</span>
              <input v-model="bookAuthor" type="text" :disabled="running" placeholder="个人归档" />
            </label>
          </div>
          <label class="field">
            <span>输出目录</span>
            <div class="path-row">
              <input v-model="outputDir" type="text" :disabled="running" placeholder="选择保存 book.md 的文件夹" />
              <button type="button" class="ghost" :disabled="running" @click="browseOutput">选择</button>
            </div>
          </label>

          <div class="checks">
            <label><input v-model="exportEpub" type="checkbox" :disabled="running" /> 导出 EPUB</label>
            <label><input v-model="localizeImages" type="checkbox" :disabled="running" /> 下载图片到本地</label>
            <label><input v-model="stripTracking" type="checkbox" :disabled="running" /> 去掉追踪参数</label>
            <label><input v-model="useCache" type="checkbox" :disabled="running" /> 使用本地缓存</label>
          </div>

          <details>
            <summary>速度和重试</summary>
            <div class="split three">
              <label class="field">
                <span>并发</span>
                <input v-model.number="concurrency" type="number" min="1" max="6" :disabled="running" />
              </label>
              <label class="field">
                <span>失败重试</span>
                <input v-model.number="retries" type="number" min="0" max="5" :disabled="running" />
              </label>
              <label class="field">
                <span>间隔（毫秒）</span>
                <input v-model.number="delayMs" type="number" min="0" max="10000" step="100" :disabled="running" />
              </label>
            </div>
          </details>

          <button type="button" class="primary" :disabled="runDisabled" @click="run">
            {{ running ? "正在归档…" : "开始归档" }}
          </button>
        </template>
      </section>

      <ProgressCard :rows="rows" :phase="phase" :running="running" :report="report" @open="openDir" />
    </main>

    <footer>
      <p class="update-row">
        <button type="button" class="ghost" :disabled="updateBusy" @click="checkUpdates(true)">检查更新</button>
        <button v-if="updateAvailable" type="button" class="ghost" :disabled="updateBusy" @click="onInstallUpdate">
          下载并安装
        </button>
        <span v-if="updateMessage">{{ updateMessage }}</span>
      </p>
      <p>只归档你登录后有权阅读的那一个公众号，默认拉取它的全部历史。不绕过付费墙，也不会抓取你打不开的号。</p>
      <p v-if="paths">日志 {{ paths.logDir }} · 缓存 {{ paths.cacheDir }}</p>
    </footer>
  </div>
</template>

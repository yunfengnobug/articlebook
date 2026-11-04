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
    note: "用你自己登录后的微信网页，拉取一个你有权阅读的公众号历史目录。不会调用 Frida 或 WMPF，也不会伪装微信客户端。",
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

function clampedHistoryLimit(): number {
  const value = Number(historyLimit.value);
  if (!Number.isFinite(value) || value <= 0) return 40;
  return Math.min(200, Math.max(1, Math.round(value)));
}

function kindLabel(kind: string): string {
  if (kind === "reader-history") return "历史消息";
  if (kind === "owner-console") return "公众平台图文";
  return kind;
}

function describeSession(value: SessionSummary | null): string {
  if (!value?.saved) {
    return "还没有会话。保存后写到应用数据目录里的 wechat-web-session.json。";
  }
  const cookie = value.hasCookie ? "已带 Cookie" : "没有 Cookie";
  const name = value.accountName || "未命名公众号";
  return `已保存${kindLabel(value.kind)} · ${name} · ${cookie} · ${value.capturedAt}`;
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
    if (!requireDesktop("预览历史目录")) return;
    try {
      const previewed = await previewWechatHistory(accountName.value, clampedHistoryLimit());
      const sample = previewed.urls.slice(0, 6).join("\n");
      previewText.value = sample ? `${previewed.sourceNote}\n${sample}` : previewed.sourceNote;
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
    pageHint.value = "在新窗口里用微信扫码登录，然后打开一个你有权阅读的公众号「历史消息」。";
  } catch (err) {
    error.value = errorText(err);
  }
}

async function openLoginSite() {
  error.value = "";
  if (!requireDesktop("打开系统浏览器")) return;
  try {
    await openWechatLoginSite();
    pageHint.value = "系统浏览器不会把 Cookie 交回应用。登录后请复制历史页地址；如果目录请求失败，再粘贴请求头里的 Cookie。";
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
      ? "会话已保存。预览目录确认篇数后，就可以开始归档。"
      : "地址已保存，但没有 Cookie。若预览失败，把请求头 Cookie 粘贴后再保存一次。";
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
    pageHint.value = "本机会话已删除。";
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
    error.value = "请先保存微信网页会话。归档读取的是已保存的会话文件。";
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
            historyLimit: clampedHistoryLimit(),
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
      当前是浏览器预览：模式和公众号来源可以切换。登录、保存会话、抓取和写 book.md 需要在桌面窗口里运行。
    </p>
    <p v-if="error" class="banner bad" role="alert">{{ error }}</p>

    <main>
      <section class="card composer">
        <header class="card-head">
          <h2>{{ mode === "list" ? "链接列表" : "公众号" }}</h2>
          <p v-if="mode === 'list'">微信文章和普通网页可以混在一起。空行和 # 注释会跳过。</p>
          <p v-else-if="wechatMode">
            用你自己登录后的微信网页，拉取一个你有权阅读的公众号历史，再走同一套归档。
          </p>
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
        <template v-else>
          <label class="field">
            <span>目录来源</span>
            <select v-model="providerId" :disabled="running">
              <option v-for="item in providers" :key="item.id" :value="item.id">
                {{ item.name }}{{ item.available ? "" : "（未实现）" }}
              </option>
            </select>
          </label>
          <p class="note">{{ selectedProvider?.note }}</p>

          <template v-if="wechatMode">
            <div class="split">
              <label class="field">
                <span>合集名称</span>
                <input v-model="accountName" type="text" :disabled="running" placeholder="例如：周末读完" />
              </label>
              <label class="field">
                <span>最多篇数</span>
                <input v-model.number="historyLimit" type="number" min="1" max="200" :disabled="running" />
              </label>
            </div>
            <label class="field">
              <span>历史页地址</span>
              <textarea
                v-model="pageUrl"
                rows="3"
                spellcheck="false"
                :disabled="running"
                placeholder="登录后打开公众号「历史消息」，或公众平台的图文列表，再把地址栏粘到这里"
                @change="describePage"
              ></textarea>
            </label>
            <details class="cookie-box">
              <summary>Cookie（可选）</summary>
              <p class="note">
                应用内登录窗口会在保存时读取 Cookie，包括 HttpOnly。系统浏览器不会自动交回 Cookie。只粘贴请求头里的一行，不要换行。
              </p>
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
            <p class="note">{{ sessionLine }}</p>
            <p v-if="session?.path" class="note">会话文件 {{ session.path }}</p>
            <p v-if="pageHint" class="note">{{ pageHint }}</p>
          </template>

          <template v-else-if="manualMode">
            <div class="split">
              <label class="field">
                <span>合集名称</span>
                <input v-model="accountName" type="text" :disabled="running" placeholder="例如：周末读完" />
              </label>
              <label class="field">
                <span>标识（可选）</span>
                <input v-model="accountId" type="text" :disabled="running" placeholder="留给以后的目录来源" />
              </label>
            </div>
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
        </template>

        <div v-if="mode === 'list' || manualMode" class="row-actions">
          <button type="button" class="ghost" :disabled="running" @click="browseLinks">从文件导入</button>
          <button type="button" class="ghost" :disabled="running" @click="preview">识别链接</button>
          <input ref="fileInput" class="hidden" type="file" accept=".txt,.md,.csv,text/plain" @change="onLocalFile" />
        </div>
        <div v-else-if="wechatMode" class="row-actions">
          <button type="button" class="ghost" :disabled="running" @click="openLoginWindow">在应用内打开登录页</button>
          <button type="button" class="ghost" :disabled="running" @click="openLoginSite">用系统浏览器打开</button>
          <button type="button" class="ghost" :disabled="running" @click="saveSession">保存会话</button>
          <button type="button" class="ghost" :disabled="running" @click="clearSession">清除会话</button>
          <button type="button" class="ghost" :disabled="running" @click="preview">预览目录</button>
        </div>
        <p v-if="previewText" class="note preview-block">{{ previewText }}</p>

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
      <p>只保存你有权阅读的文章。ArticleBook 不绕过付费墙，也不批量抓取公众号。</p>
      <p v-if="paths">日志 {{ paths.logDir }} · 缓存 {{ paths.cacheDir }}</p>
    </footer>
  </div>
</template>

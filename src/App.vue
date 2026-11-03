<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import ModeSwitch from "./components/ModeSwitch.vue";
import ProgressCard from "./components/ProgressCard.vue";
import {
  chooseDirectory,
  chooseLinksFile,
  errorText,
  readTextFile,
  inTauri,
  listenProgress,
  loadPaths,
  loadProviders,
  loadSuggestedDir,
  openOutput,
  previewLinks,
  runAccountExport,
  runListExport,
} from "./desktop";
import type { AppPaths, ExportSettings, JobReport, Mode, ProgressEvent, ProgressRow, ProviderInfo } from "./types";

const desktop = ref(inTauri());
const mode = ref<Mode>("list");
const urlsText = ref("");
const accountName = ref("");
const accountId = ref("");
const linksText = ref("");
const linksPath = ref("");
const providerId = ref("manual");
const providers = ref<ProviderInfo[]>([
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
]);
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
const fileInput = ref<HTMLInputElement | null>(null);
let unlisten: (() => void) | null = null;

const selectedProvider = computed(
  () => providers.value.find((item) => item.id === providerId.value) ?? providers.value[0],
);
const providerBlocked = computed(() => mode.value === "account" && selectedProvider.value?.available === false);
const runDisabled = computed(() => running.value || providerBlocked.value);

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

async function preview() {
  error.value = "";
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
  if (mode.value === "account" && !linksText.value.trim() && !linksPath.value.trim()) {
    error.value = "请粘贴链接，或选择链接文件。";
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
            ...body,
          });
    phase.value = `完成：成功 ${report.value.succeeded}，失败 ${report.value.failed}`;
  } catch (err) {
    error.value = errorText(err);
  } finally {
    running.value = false;
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
    unlisten = await listenProgress(onProgress);
    paths.value = await loadPaths();
    const suggested = await loadSuggestedDir();
    if (suggested && !outputDir.value) outputDir.value = suggested;
    const remote = await loadProviders();
    if (remote && remote.length > 0) providers.value = remote;
  } catch (err) {
    error.value = errorText(err);
  }
});

onUnmounted(() => {
  unlisten?.();
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
      当前是浏览器预览：模式可以切换。抓取、写 book.md 和 EPUB 需要在桌面窗口里运行。
    </p>
    <p v-if="error" class="banner bad" role="alert">{{ error }}</p>

    <main>
      <section class="card composer">
        <header class="card-head">
          <h2>{{ mode === "list" ? "链接列表" : "公众号" }}</h2>
          <p v-if="mode === 'list'">微信文章和普通网页可以混在一起。空行和 # 注释会跳过。</p>
          <p v-else>本期只支持手动导入链接。不会登录微信，也不会调用 WMPF / Frida。</p>
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
              :disabled="running || providerBlocked"
              placeholder="每行一条，或从文件导入"
            ></textarea>
          </label>
          <p v-if="linksPath" class="note">将读取文件：{{ linksPath }}</p>
        </template>

        <div class="row-actions">
          <button type="button" class="ghost" :disabled="running" @click="browseLinks">从文件导入</button>
          <button type="button" class="ghost" :disabled="running" @click="preview">识别链接</button>
          <input ref="fileInput" class="hidden" type="file" accept=".txt,.md,.csv,text/plain" @change="onLocalFile" />
        </div>
        <p v-if="previewText" class="note">{{ previewText }}</p>

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
      <p>只保存你有权阅读的文章。ArticleBook 不绕过付费墙，也不批量抓取公众号。</p>
      <p v-if="paths">日志 {{ paths.logDir }} · 缓存 {{ paths.cacheDir }}</p>
    </footer>
  </div>
</template>

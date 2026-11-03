<script setup lang="ts">
import type { JobReport, ProgressRow } from "../types";

defineProps<{
  rows: ProgressRow[];
  phase: string;
  running: boolean;
  report: JobReport | null;
}>();

const emit = defineEmits<{ open: [] }>();

const labels: Record<string, string> = {
  fetching: "抓取",
  retrying: "重试",
  cached: "缓存",
  ok: "完成",
  failed: "失败",
  images: "图片",
};
</script>

<template>
  <section class="card progress-card" aria-live="polite">
    <header class="card-head">
      <h2>进度</h2>
      <p v-if="running">{{ phase || "正在处理…" }}</p>
      <p v-else-if="report">成功 {{ report.succeeded }} · 失败 {{ report.failed }} · 缓存 {{ report.cached }}</p>
      <p v-else>尚未运行</p>
    </header>

    <p v-if="rows.length === 0" class="empty">
      粘贴链接并选择输出目录后开始。失败的链接会跳过，其余文章继续写入 book.md。
    </p>
    <ol v-else class="rows">
      <li v-for="row in rows" :key="row.index" :data-status="row.status">
        <span class="badge">{{ labels[row.status] ?? row.status }}</span>
        <div>
          <strong>{{ row.title || row.url }}</strong>
          <p>{{ row.message }}</p>
          <p v-if="row.title" class="url">{{ row.url }}</p>
        </div>
      </li>
    </ol>

    <div v-if="report" class="result">
      <p>书稿：{{ report.bookMarkdown }}</p>
      <p v-if="report.bookEpub">EPUB：{{ report.bookEpub }}</p>
      <p>失败清单：{{ report.failuresMarkdown }}</p>
      <ul v-if="report.articles.length" class="titles">
        <li v-for="article in report.articles" :key="article.url">
          {{ article.title }} <span>{{ article.kind }}</span>
        </li>
      </ul>
      <ul v-if="report.failures.length" class="failures">
        <li v-for="failure in report.failures" :key="failure.url">
          {{ failure.url }} — {{ failure.error }}
        </li>
      </ul>
      <ul v-if="report.warnings.length" class="warnings">
        <li v-for="warning in report.warnings" :key="warning">{{ warning }}</li>
      </ul>
      <button type="button" class="ghost" @click="emit('open')">打开输出目录</button>
    </div>
  </section>
</template>

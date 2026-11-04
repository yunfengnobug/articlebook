import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import type {
  AppPaths,
  HistoryPreview,
  JobReport,
  PageHint,
  ParsedUrls,
  ProgressEvent,
  ProviderInfo,
  SessionSummary,
  WechatPageEvent,
} from "./types";

export function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error && error.message) return error.message;
  return "导出失败";
}

export async function loadPaths(): Promise<AppPaths | null> {
  if (!inTauri()) return null;
  return invoke<AppPaths>("app_paths");
}

export async function loadSuggestedDir(): Promise<string | null> {
  if (!inTauri()) return null;
  return invoke<string>("suggest_output_dir");
}

export async function loadProviders(): Promise<ProviderInfo[] | null> {
  if (!inTauri()) return null;
  return invoke<ProviderInfo[]>("list_providers");
}

export async function previewLinks(text: string, stripTracking: boolean): Promise<ParsedUrls> {
  if (!inTauri()) {
    const urls = text.match(/https?:\/\/[^\s<>"'“”]+/g) ?? [];
    return { urls, invalid: [], duplicates: 0 };
  }
  return invoke<ParsedUrls>("preview_links", { text, stripTracking });
}

export async function chooseDirectory(): Promise<string | null> {
  const selected = await open({
    directory: true,
    multiple: false,
    title: "选择输出目录",
  });
  return onePath(selected);
}

export async function readTextFile(path: string): Promise<string> {
  return invoke<string>("read_text_file", { path });
}

export async function chooseLinksFile(): Promise<string | null> {
  const selected = await open({
    directory: false,
    multiple: false,
    title: "选择链接文件",
    filters: [{ name: "文本", extensions: ["txt", "md", "csv"] }],
  });
  return onePath(selected);
}

export async function runListExport(req: Record<string, unknown>): Promise<JobReport> {
  return invoke<JobReport>("run_list_export", { req });
}

export async function runAccountExport(req: Record<string, unknown>): Promise<JobReport> {
  return invoke<JobReport>("run_account_export", { req });
}

export async function openOutput(path: string): Promise<void> {
  await invoke("open_output_dir", { path });
}

export function listenProgress(handler: (event: ProgressEvent) => void): Promise<UnlistenFn> {
  return listen<ProgressEvent>("job-progress", (event) => handler(event.payload));
}

export function listenWechatPage(handler: (event: WechatPageEvent) => void): Promise<UnlistenFn> {
  return listen<WechatPageEvent>("wechat-page", (event) => handler(event.payload));
}

export async function describeWechatPage(url: string): Promise<PageHint> {
  return invoke<PageHint>("describe_wechat_page_url", { url });
}

export async function loadWechatSession(): Promise<SessionSummary> {
  return invoke<SessionSummary>("load_wechat_session");
}

export async function saveWechatSession(req: {
  pageUrl: string;
  cookie: string;
  accountName: string;
  clearCookie: boolean;
}): Promise<SessionSummary> {
  return invoke<SessionSummary>("save_wechat_session", { req });
}

export async function clearWechatSession(): Promise<SessionSummary> {
  return invoke<SessionSummary>("clear_wechat_session");
}

export async function openWechatLoginWindow(): Promise<void> {
  await invoke("open_wechat_login_window");
}

export async function openWechatLoginSite(): Promise<void> {
  await invoke("open_wechat_login_site");
}

export async function previewWechatHistory(
  accountName: string,
  historyLimit: number,
): Promise<HistoryPreview> {
  return invoke<HistoryPreview>("preview_wechat_history", { accountName, historyLimit });
}

function onePath(value: unknown): string | null {
  if (typeof value === "string" && value.length > 0) return value;
  if (Array.isArray(value) && typeof value[0] === "string") return value[0];
  return null;
}

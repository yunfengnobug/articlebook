import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import type { AppPaths, JobReport, ParsedUrls, ProgressEvent, ProviderInfo } from "./types";

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

function onePath(value: unknown): string | null {
  if (typeof value === "string" && value.length > 0) return value;
  if (Array.isArray(value) && typeof value[0] === "string") return value[0];
  return null;
}

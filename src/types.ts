export type Mode = "list" | "account";

export interface ProviderInfo {
  id: string;
  name: string;
  available: boolean;
  note: string;
}

export interface ParsedUrls {
  urls: string[];
  invalid: string[];
  duplicates: number;
}

export interface ProgressEvent {
  jobId: string;
  phase: string;
  index: number;
  total: number;
  url: string;
  title: string | null;
  status: string;
  message: string;
  attempt: number;
}

export interface FailureRecord {
  url: string;
  error: string;
  attempts: number;
}

export interface ArticleSummary {
  title: string;
  url: string;
  cached: boolean;
  kind: string;
}

export interface JobReport {
  jobId: string;
  outputDir: string;
  bookMarkdown: string;
  bookEpub: string | null;
  failuresMarkdown: string;
  succeeded: number;
  failed: number;
  cached: number;
  failures: FailureRecord[];
  articles: ArticleSummary[];
  warnings: string[];
}

export interface AppPaths {
  dataDir: string;
  cacheDir: string;
  logDir: string;
}

export interface ExportSettings {
  outputDir: string;
  bookTitle: string;
  bookAuthor: string;
  localizeImages: boolean;
  exportEpub: boolean;
  stripTracking: boolean;
  concurrency: number;
  retries: number;
  delayMs: number;
  useCache: boolean;
}

export interface ProgressRow {
  index: number;
  total: number;
  url: string;
  title: string | null;
  status: string;
  message: string;
  attempt: number;
}

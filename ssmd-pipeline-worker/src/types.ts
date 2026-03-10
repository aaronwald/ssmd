export type StageType = "sql" | "http" | "gcs_check" | "openrouter" | "email";
export type RunStatus = "pending" | "running" | "completed" | "failed";

export interface StageConfig {
  query?: string;
  max_rows?: number;
  url?: string;
  method?: string;
  headers?: Record<string, string>;
  body?: string;
  path?: string;
  model?: string;
  prompt?: string;
  system_prompt?: string;
  user_prompt?: string;
  max_tokens?: number;
  temperature?: number;
  to?: string;
  subject?: string;
  template?: string;
  html?: string;
  timeout_ms?: number;
}

export interface StageResult {
  status: "completed" | "failed";
  output?: unknown;
  error?: string;
}

export const DEFAULT_TIMEOUTS: Record<StageType, number> = {
  sql: 30_000,
  http: 30_000,
  gcs_check: 15_000,
  openrouter: 120_000,
  email: 15_000,
};

export const MAX_OUTPUT_SIZE = 65_536;
export const DEFAULT_MAX_ROWS = 100;

export const HTTP_URL_ALLOWLIST = [
  "http://ssmd-data-ts-internal:8081/",
  "http://ssmd-data-ts-internal.ssmd.svc.cluster.local:8081/",
];

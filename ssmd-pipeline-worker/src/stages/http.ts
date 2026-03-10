import type { StageConfig, StageResult } from "../types.ts";
import { HTTP_URL_ALLOWLIST, MAX_OUTPUT_SIZE } from "../types.ts";
import type { ExecuteContext } from "./mod.ts";

export function validateUrl(url: string, allowlist: string[]): boolean {
  return allowlist.some((prefix) => url.startsWith(prefix));
}

export async function executeHttp(
  config: StageConfig,
  _ctx: ExecuteContext,
  signal: AbortSignal,
): Promise<StageResult> {
  const url = config.url;
  if (!url) {
    return { status: "failed", error: "HTTP stage requires 'url' in config" };
  }

  if (!validateUrl(url, HTTP_URL_ALLOWLIST)) {
    return {
      status: "failed",
      error: `URL not in allowlist. Allowed prefixes: ${HTTP_URL_ALLOWLIST.join(", ")}`,
    };
  }

  const method = (config.method ?? "GET").toUpperCase();
  const headers: Record<string, string> = { ...config.headers };

  try {
    const fetchOpts: RequestInit = {
      method,
      headers,
      signal,
      redirect: "error",
    };

    if (config.body && method !== "GET") {
      fetchOpts.body = config.body;
      if (!headers["content-type"]) {
        headers["content-type"] = "application/json";
      }
    }

    const resp = await fetch(url, fetchOpts);
    const text = await resp.text();

    const truncated = text.length > MAX_OUTPUT_SIZE;
    const body = truncated ? text.slice(0, MAX_OUTPUT_SIZE) : text;

    let data: unknown;
    try {
      data = JSON.parse(body);
    } catch {
      data = body;
    }

    if (!resp.ok) {
      return {
        status: "failed",
        error: `HTTP ${resp.status}: ${typeof data === "string" ? data.slice(0, 500) : JSON.stringify(data).slice(0, 500)}`,
        output: { status: resp.status, body: data, truncated },
      };
    }

    return {
      status: "completed",
      output: { status: resp.status, body: data, truncated },
    };
  } catch (err) {
    return {
      status: "failed",
      error: err instanceof Error ? err.message : String(err),
    };
  }
}

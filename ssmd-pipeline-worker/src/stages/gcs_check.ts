import type { StageConfig, StageResult } from "../types.ts";
import type { ExecuteContext } from "./mod.ts";

export async function executeGcsCheck(
  config: StageConfig,
  ctx: ExecuteContext,
  signal: AbortSignal,
): Promise<StageResult> {
  const path = config.path;
  if (!path) {
    return { status: "failed", error: "gcs_check stage requires 'path' in config" };
  }

  try {
    const url = `${ctx.dataTsUrl}/v1/gcs/check?path=${encodeURIComponent(path)}`;
    const resp = await fetch(url, {
      method: "GET",
      headers: { "Authorization": `Bearer ${ctx.adminApiKey}` },
      signal,
    });

    if (!resp.ok) {
      const text = await resp.text();
      return {
        status: "completed",
        output: { exists: false, path, error: text },
      };
    }

    const data = await resp.json();
    return {
      status: "completed",
      output: { exists: true, path, ...data },
    };
  } catch (err) {
    return {
      status: "failed",
      error: err instanceof Error ? err.message : String(err),
    };
  }
}

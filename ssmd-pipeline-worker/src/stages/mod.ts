import type { StageType, StageConfig, StageResult } from "../types.ts";
import { DEFAULT_TIMEOUTS } from "../types.ts";

export interface ExecuteContext {
  readonlySql: unknown; // postgres connection, typed properly when wired
  dataTsUrl: string;
  adminApiKey: string;
}

type StageHandler = (config: StageConfig, ctx: ExecuteContext, signal: AbortSignal) => Promise<StageResult>;

const handlers = new Map<StageType, StageHandler>();

export function registerHandler(type: StageType, handler: StageHandler): void {
  handlers.set(type, handler);
}

export async function executeStage(
  stageType: StageType,
  config: StageConfig,
  ctx: ExecuteContext,
): Promise<StageResult> {
  const handler = handlers.get(stageType);
  if (!handler) {
    return { status: "failed", error: `Unknown stage type: ${stageType}` };
  }

  const timeoutMs = config.timeout_ms ?? DEFAULT_TIMEOUTS[stageType];
  const signal = AbortSignal.timeout(timeoutMs);

  try {
    return await handler(config, ctx, signal);
  } catch (err) {
    if (err instanceof DOMException && err.name === "TimeoutError") {
      return { status: "failed", error: `Stage timed out after ${timeoutMs}ms` };
    }
    return { status: "failed", error: err instanceof Error ? err.message : String(err) };
  }
}

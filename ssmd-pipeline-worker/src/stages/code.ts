import type { StageConfig, StageResult } from "../types.ts";
import type { ExecuteContext } from "./mod.ts";
import { codeFunctions } from "../functions/mod.ts";
import type { CodeInput } from "../functions/mod.ts";

export async function executeCode(
  config: StageConfig,
  _ctx: ExecuteContext,
  _signal: AbortSignal,
): Promise<StageResult> {
  const fnName = config.function;
  if (!fnName) {
    return { status: "failed", error: "Code stage requires 'function' in config" };
  }

  const fn = codeFunctions[fnName];
  if (!fn) {
    return {
      status: "failed",
      error: `Unknown code function: ${fnName}. Available: ${Object.keys(codeFunctions).join(", ")}`,
    };
  }

  const templateCtx = config._context as CodeInput | undefined;
  if (!templateCtx) {
    return { status: "failed", error: "Code stage requires '_context' injected by worker" };
  }

  const input: CodeInput = {
    stages: templateCtx.stages,
    triggerInfo: templateCtx.triggerInfo,
    date: templateCtx.date,
    params: config.params,
  };

  try {
    const output = fn(input);
    return { status: "completed", output };
  } catch (err) {
    return {
      status: "failed",
      error: err instanceof Error ? err.message : String(err),
    };
  }
}

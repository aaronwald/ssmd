import postgres from "postgres";
import type { Sql } from "postgres";

import { registerHandler, executeStage } from "./stages/mod.ts";
import type { ExecuteContext } from "./stages/mod.ts";
import { executeSql } from "./stages/sql.ts";
import { executeHttp } from "./stages/http.ts";
import { executeGcsCheck } from "./stages/gcs_check.ts";
import { executeOpenRouter } from "./stages/openrouter.ts";
import { executeEmail } from "./stages/email.ts";
import { executeCode } from "./stages/code.ts";
import { resolveTemplate } from "./template.ts";
import type { TemplateContext } from "./template.ts";
import type { StageConfig } from "./types.ts";

import {
  claimNextRun,
  loadStages,
  insertStageResult,
  finishRun,
  loadCronPipelines,
  markCronTriggered,
  insertRun,
} from "./db.ts";
import type { PipelineRun, PipelineStage } from "./db.ts";
import { computeCronDate, isCronDue } from "./cron.ts";

// ── Configuration ───────────────────────────────────────────────

export interface WorkerConfig {
  databaseUrl: string;
  databaseUrlReadonly: string;
  dataTsUrl: string;
  adminApiKey: string;
  pollIntervalMs: number;
}

// ── Worker lifecycle ────────────────────────────────────────────

let running = false;

export async function startWorker(config: WorkerConfig): Promise<void> {
  // Register all stage handlers
  registerHandler("sql", executeSql);
  registerHandler("http", executeHttp);
  registerHandler("gcs_check", executeGcsCheck);
  registerHandler("openrouter", executeOpenRouter);
  registerHandler("email", (cfg, ctx, signal) => executeEmail(cfg, ctx, signal, ctx.pipelineId?.toString()));
  registerHandler("code", executeCode);

  // Create postgres connections
  const sql = postgres(config.databaseUrl, {
    max: 2,
    idle_timeout: 30,
    connect_timeout: 10,
  });

  const readonlySql = postgres(config.databaseUrlReadonly, {
    max: 2,
    idle_timeout: 30,
    connect_timeout: 10,
  });

  const ctx: ExecuteContext = {
    readonlySql,
    dataTsUrl: config.dataTsUrl,
    adminApiKey: config.adminApiKey,
  };

  running = true;

  // Graceful shutdown
  const shutdown = () => {
    console.log("[worker] shutting down...");
    running = false;
  };
  Deno.addSignalListener("SIGINT", shutdown);
  Deno.addSignalListener("SIGTERM", shutdown);

  console.log(`[worker] started — polling every ${config.pollIntervalMs}ms`);

  while (running) {
    try {
      // Phase 1: Evaluate cron schedules and insert pending runs
      await evaluateCronSchedules(sql);

      // Phase 2: Claim and execute pending runs
      let claimed = true;
      while (claimed && running) {
        const run = await claimNextRun(sql);
        if (!run) {
          claimed = false;
          break;
        }
        await executeRun(sql, run, ctx);
      }
    } catch (err) {
      console.error("[worker] poll loop error:", err instanceof Error ? err.message : String(err));
    }

    // Sleep between poll cycles
    if (running) {
      await sleep(config.pollIntervalMs);
    }
  }

  // Cleanup connections
  console.log("[worker] closing database connections...");
  await sql.end();
  await readonlySql.end();
  console.log("[worker] stopped");
}

// ── Cron evaluation ─────────────────────────────────────────────

async function evaluateCronSchedules(sql: Sql): Promise<void> {
  const cronPipelines = await loadCronPipelines(sql);
  const now = new Date();

  for (const pipeline of cronPipelines) {
    if (!isCronDue(pipeline, now)) continue;

    // Atomic dedup: only insert run if we successfully mark as triggered
    const claimed = await markCronTriggered(sql, pipeline.id, now);
    if (!claimed) continue;

    const triggerConfig = pipeline.trigger_config as {
      schedule?: string;
      date_offset_days?: number;
    };
    const date = computeCronDate(triggerConfig, now);

    const runId = await insertRun(sql, pipeline.id, {
      trigger: "cron",
      schedule: triggerConfig.schedule,
      triggered_at: now.toISOString(),
      date,
    });
    console.log(`[worker] cron triggered pipeline=${pipeline.name} run=${runId}`);
  }
}

// ── Run execution ───────────────────────────────────────────────

async function executeRun(
  sql: Sql,
  run: PipelineRun,
  ctx: ExecuteContext,
): Promise<void> {
  console.log(`[worker] executing run=${run.id} pipeline=${run.pipeline_id}`);

  // Set pipelineId on context for per-pipeline rate limiting (e.g., email)
  ctx.pipelineId = run.pipeline_id;

  const stages = await loadStages(sql, run.pipeline_id);
  if (stages.length === 0) {
    console.warn(`[worker] run=${run.id} has no stages, marking completed`);
    await finishRun(sql, run.id, "completed");
    return;
  }

  // Build template context that accumulates stage outputs
  const triggerInfo = run.trigger_info ?? {};
  const templateCtx: TemplateContext = {
    input: "",
    stages: {},
    triggerInfo,
    date: typeof triggerInfo.date === "string"
      ? triggerInfo.date
      : new Date(Date.now() - 86_400_000).toISOString().slice(0, 10),
  };

  let runFailed = false;

  for (const stage of stages) {
    if (!running) {
      // Graceful shutdown — mark run as failed so it can be retried
      await finishRun(sql, run.id, "failed");
      return;
    }

    const stageStarted = new Date();

    // Normalize config: handle double-encoded JSONB (string instead of object)
    let stageConfig = stage.config;
    if (typeof stageConfig === "string") {
      try {
        stageConfig = JSON.parse(stageConfig);
      } catch {
        // leave as-is, stage handler will report the error
      }
    }

    // Resolve templates in stage config
    const resolvedConfig = resolveStageConfig(stageConfig as StageConfig, templateCtx);

    // Inject template context for code stages so functions can access previous stage outputs
    if (stage.stage_type === "code") {
      resolvedConfig._context = templateCtx;
    }

    // Execute the stage
    const result = await executeStage(stage.stage_type, resolvedConfig, ctx);

    const stageFinished = new Date();

    // Strip _context before persisting — it may contain webhook secrets
    const persistConfig = stripContextFromConfig(resolvedConfig);

    // Record stage result
    await insertStageResult(
      sql,
      run.id,
      stage.id,
      result.status,
      persistConfig,
      result.output ?? null,
      result.error ?? null,
      stageStarted,
      stageFinished,
    );

    // Update template context with stage output
    const outputStr = result.output !== undefined ? JSON.stringify(result.output) : "";
    templateCtx.stages[stage.position] = { output: outputStr };

    if (result.status === "failed") {
      console.error(
        `[worker] run=${run.id} stage=${stage.position}/${stage.name} failed: ${result.error}`,
      );
      runFailed = true;
      break;
    }

    // Check if code stage signaled skip — mark remaining stages as skipped
    const shouldSkip = result.output != null &&
      typeof result.output === "object" &&
      (result.output as Record<string, unknown>).skip === true;

    if (shouldSkip) {
      console.log(
        `[worker] run=${run.id} stage=${stage.position}/${stage.name} signaled skip`,
      );
      const currentIdx = stages.indexOf(stage);
      for (const remaining of stages.slice(currentIdx + 1)) {
        const skipTime = new Date();
        await insertStageResult(
          sql,
          run.id,
          remaining.id,
          "skipped",
          remaining.config as StageConfig,
          null,
          null,
          skipTime,
          skipTime,
        );
      }
      break;
    }

    console.log(
      `[worker] run=${run.id} stage=${stage.position}/${stage.name} completed`,
    );
  }

  await finishRun(sql, run.id, runFailed ? "failed" : "completed");
  console.log(`[worker] run=${run.id} finished status=${runFailed ? "failed" : "completed"}`);
}

// ── Security helpers ───────────────────────────────────────────

/** Strip _context before persisting — contains triggerInfo which may have secrets */
export function stripContextFromConfig(config: StageConfig): StageConfig {
  if (!("_context" in config)) return config;
  const { _context: _, ...rest } = config;
  return rest as StageConfig;
}

// ── Template resolution for stage config ────────────────────────

function resolveStageConfig(config: StageConfig, ctx: TemplateContext): StageConfig {
  const resolved: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(config)) {
    if (typeof value === "string") {
      resolved[key] = resolveTemplate(value, ctx);
    } else {
      resolved[key] = value;
    }
  }
  return resolved as StageConfig;
}

// ── Helpers ─────────────────────────────────────────────────────

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

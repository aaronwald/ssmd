import type { Sql } from "postgres";
import type { StageType, RunStatus } from "./types.ts";

// ── Row shapes returned by queries ──────────────────────────────

export interface PipelineRun {
  id: number;
  pipeline_id: number;
  status: RunStatus;
  trigger_info: Record<string, unknown> | null;
  created_at: string;
}

export interface PipelineStage {
  id: number;
  pipeline_id: number;
  position: number;
  name: string;
  stage_type: StageType;
  config: Record<string, unknown>;
}

export interface CronPipeline {
  id: number;
  name: string;
  trigger_config: Record<string, unknown>;
  last_triggered_at: string | null;
}

// ── Claim next pending run (FOR UPDATE SKIP LOCKED) ─────────────

export async function claimNextRun(sql: Sql): Promise<PipelineRun | null> {
  const rows = await sql`
    UPDATE pipeline_runs
    SET status = 'running', started_at = NOW()
    WHERE id = (
      SELECT pr.id
      FROM pipeline_runs pr
      JOIN pipeline_definitions pd ON pd.id = pr.pipeline_id
      WHERE pr.status = 'pending' AND pd.enabled = true
      ORDER BY pr.created_at ASC
      LIMIT 1
      FOR UPDATE OF pr SKIP LOCKED
    )
    RETURNING
      id,
      pipeline_id,
      status,
      trigger_info,
      created_at::text
  `;

  if (rows.length === 0) return null;
  return rows[0] as unknown as PipelineRun;
}

// ── Load ordered stages for a pipeline ──────────────────────────

export async function loadStages(sql: Sql, pipelineId: number): Promise<PipelineStage[]> {
  const rows = await sql`
    SELECT id, pipeline_id, position, name, stage_type, config
    FROM pipeline_stages
    WHERE pipeline_id = ${pipelineId}
    ORDER BY position ASC
  `;
  return rows as unknown as PipelineStage[];
}

// ── Insert a stage result ───────────────────────────────────────

export async function insertStageResult(
  sql: Sql,
  runId: number,
  stageId: number,
  status: "completed" | "failed",
  input: unknown,
  output: unknown,
  error: string | null,
  startedAt: Date,
  finishedAt: Date,
): Promise<void> {
  await sql`
    INSERT INTO pipeline_stage_results (run_id, stage_id, status, input, output, error, started_at, finished_at)
    VALUES (
      ${runId},
      ${stageId},
      ${status},
      ${JSON.stringify(input ?? null)}::jsonb,
      ${JSON.stringify(output ?? null)}::jsonb,
      ${error},
      ${startedAt.toISOString()}::timestamptz,
      ${finishedAt.toISOString()}::timestamptz
    )
  `;
}

// ── Finish a run ────────────────────────────────────────────────

export async function finishRun(
  sql: Sql,
  runId: number,
  status: "completed" | "failed",
): Promise<void> {
  await sql`
    UPDATE pipeline_runs
    SET status = ${status}, finished_at = NOW()
    WHERE id = ${runId}
  `;
}

// ── Load enabled cron pipelines ─────────────────────────────────

export async function loadCronPipelines(sql: Sql): Promise<CronPipeline[]> {
  const rows = await sql`
    SELECT id, name, trigger_config, last_triggered_at::text
    FROM pipeline_definitions
    WHERE trigger_type = 'cron' AND enabled = true
  `;
  return rows as unknown as CronPipeline[];
}

// ── Atomic cron dedup — only marks triggered if last_triggered_at is before threshold ──

export async function markCronTriggered(
  sql: Sql,
  pipelineId: number,
  beforeTime: Date,
): Promise<boolean> {
  const rows = await sql`
    UPDATE pipeline_definitions
    SET last_triggered_at = NOW()
    WHERE id = ${pipelineId}
      AND (last_triggered_at IS NULL OR last_triggered_at < ${beforeTime.toISOString()}::timestamptz)
    RETURNING id
  `;
  return rows.length > 0;
}

// ── Insert a new pending run ────────────────────────────────────

export async function insertRun(
  sql: Sql,
  pipelineId: number,
  triggerInfo: Record<string, unknown>,
): Promise<number> {
  const rows = await sql`
    INSERT INTO pipeline_runs (pipeline_id, status, trigger_info)
    VALUES (${pipelineId}, 'pending', ${JSON.stringify(triggerInfo)}::jsonb)
    RETURNING id
  `;
  return (rows[0] as unknown as { id: number }).id;
}

import type { StageConfig, StageResult } from "../types.ts";
import { DEFAULT_MAX_ROWS } from "../types.ts";
import type { ExecuteContext } from "./mod.ts";

const FORBIDDEN_PATTERNS = /^\s*(DELETE|DROP|UPDATE|INSERT|TRUNCATE|ALTER|CREATE|GRANT|REVOKE|DO\s+\$)\b/i;
const SEMICOLON_CHAIN = /;\s*(DELETE|DROP|UPDATE|INSERT|TRUNCATE|ALTER|CREATE|GRANT|REVOKE|DO\s+\$)\b/i;

export function validateSqlQuery(query: string): boolean {
  if (FORBIDDEN_PATTERNS.test(query)) return false;
  if (SEMICOLON_CHAIN.test(query)) return false;
  return true;
}

export function truncateRows(
  rows: Record<string, unknown>[],
  maxRows: number,
): { rows: Record<string, unknown>[]; truncated: boolean; totalRows: number } {
  const totalRows = rows.length;
  if (totalRows <= maxRows) {
    return { rows, truncated: false, totalRows };
  }
  return { rows: rows.slice(0, maxRows), truncated: true, totalRows };
}

export async function executeSql(
  config: StageConfig,
  ctx: ExecuteContext,
  _signal: AbortSignal,
): Promise<StageResult> {
  const query = config.query;
  if (!query) {
    return { status: "failed", error: "SQL stage requires 'query' in config" };
  }

  if (!validateSqlQuery(query)) {
    return { status: "failed", error: "SQL query rejected: only SELECT/WITH queries are allowed" };
  }

  const maxRows = config.max_rows ?? DEFAULT_MAX_ROWS;

  try {
    // deno-lint-ignore no-explicit-any
    const sql = ctx.readonlySql as any;
    const rows = await sql.unsafe(query);
    const result = truncateRows(rows as Record<string, unknown>[], maxRows);

    return {
      status: "completed",
      output: result,
    };
  } catch (err) {
    return {
      status: "failed",
      error: err instanceof Error ? err.message : String(err),
    };
  }
}

import type { DuckDBConnection } from "@duckdb/node-api";

/**
 * Bound DuckDB's buffer pool below the container cgroup limit and point its
 * spill directory at the writable /tmp emptyDir.
 *
 * Without this, DuckDB sizes its buffer pool from HOST RAM (it reads
 * /proc/meminfo, which reflects the node, not the pod's cgroup limit) and gets
 * OOM-killed (exit 137) on large single-day aggregates — this is what broke the
 * binance WS aggregate on the first full day (2026-06-30) at a 1Gi limit.
 *
 * Call immediately after connect(), before any heavy query. `memoryLimit` MUST
 * be strictly below the container's memory limit, and `tempDir` MUST be a
 * writable path (the /tmp emptyDir) so spilling to disk succeeds on a read-only
 * root filesystem.
 */
export async function configureDuckDBLimits(
  conn: DuckDBConnection,
  opts: { memoryLimit?: string; tempDir?: string } = {},
): Promise<void> {
  const memoryLimit = opts.memoryLimit ?? "1500MB";
  const tempDir = opts.tempDir ?? "/tmp/duckdb-spill";
  if (!memoryLimit.trim()) {
    throw new Error("configureDuckDBLimits: memoryLimit must be non-empty");
  }
  if (!tempDir.trim()) {
    throw new Error("configureDuckDBLimits: tempDir must be non-empty");
  }
  // memoryLimit/tempDir are trusted, internally-built literals (never user
  // input); DuckDB SET values must be literals, not bindable parameters.
  await conn.run(`SET memory_limit='${memoryLimit}'`);
  await conn.run(`SET temp_directory='${tempDir}'`);
}

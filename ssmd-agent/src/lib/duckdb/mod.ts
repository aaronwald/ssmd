/**
 * DuckDB connection lifecycle for server-side parquet queries.
 * Uses @duckdb/node-api for N-API bindings via Deno's Node compat.
 */
import { DuckDBInstance } from "@duckdb/node-api";

let instance: DuckDBInstance | null = null;
let connection: Awaited<ReturnType<DuckDBInstance["connect"]>> | null = null;

/**
 * Initialize DuckDB: create instance, load httpfs, configure GCS.
 * Call once at server startup.
 */
export async function initDuckDB(): Promise<void> {
  instance = await DuckDBInstance.create();
  connection = await instance.connect();

  // Configure memory limit and extension directory (writable /tmp for read-only rootfs)
  await connection.run("SET memory_limit='512MB'");
  await connection.run("SET extension_directory='/tmp/duckdb_ext'");
  await connection.run("SET temp_directory='/tmp/duckdb_tmp'");

  // Load httpfs for GCS access
  await connection.run("INSTALL httpfs");
  await connection.run("LOAD httpfs");

  // Configure for GCS (DuckDB uses S3 protocol with GCS endpoint)
  await connection.run("SET s3_endpoint='storage.googleapis.com'");
  await connection.run("SET s3_url_style='path'");

  // Use GCS credential chain (Workload Identity in GKE)
  await connection.run("CREATE SECRET (TYPE GCS, PROVIDER CREDENTIAL_CHAIN)");

  console.log("DuckDB initialized with GCS access");
}

export interface QueryResult {
  columns: string[];
  rows: Record<string, unknown>[];
}

/**
 * Execute a SQL query and return results as column names + row objects.
 */
export async function query(sql: string): Promise<QueryResult> {
  if (!connection) {
    throw new Error("DuckDB not initialized. Call initDuckDB() first.");
  }

  const result = await connection.run(sql);
  const columns: string[] = [];
  const columnCount = result.columnCount;
  for (let i = 0; i < columnCount; i++) {
    columns.push(result.columnName(i));
  }

  const rows: Record<string, unknown>[] = [];
  const chunks = await result.fetchAllChunks();
  for (const chunk of chunks) {
    const rowCount = chunk.rowCount;
    for (let r = 0; r < rowCount; r++) {
      const row: Record<string, unknown> = {};
      for (let c = 0; c < columnCount; c++) {
        row[columns[c]] = chunk.getColumnVector(c).getItem(r);
      }
      rows.push(row);
    }
  }

  return { columns, rows };
}

/**
 * Close DuckDB connection and instance.
 * Call on server shutdown.
 */
export async function closeDuckDB(): Promise<void> {
  if (connection) {
    connection.close();
    connection = null;
  }
  if (instance) {
    // DuckDBInstance doesn't have explicit close, just null the reference
    instance = null;
  }
  console.log("DuckDB closed");
}

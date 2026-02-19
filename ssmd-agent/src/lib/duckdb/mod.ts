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

  // Get access token from GKE metadata server (Workload Identity)
  // DuckDB's CREDENTIAL_CHAIN doesn't work with metadata server, so fetch token directly
  try {
    const tokenResp = await fetch(
      "http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token",
      { headers: { "Metadata-Flavor": "Google" } },
    );
    const { access_token } = await tokenResp.json();
    await connection.run(`SET s3_access_key_id=''`);
    await connection.run(`SET s3_secret_access_key=''`);
    await connection.run(`SET s3_session_token='${access_token}'`);
    console.log("DuckDB initialized with GCS access (metadata server token)");
  } catch {
    // Fall back to credential chain (works with ADC file / gcloud auth locally)
    try {
      await connection.run("CREATE SECRET (TYPE GCS, PROVIDER CREDENTIAL_CHAIN)");
      console.log("DuckDB initialized with GCS access (credential chain)");
    } catch (e) {
      console.warn("DuckDB GCS auth failed, queries may not work:", (e as Error).message);
    }
  }
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

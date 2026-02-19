/**
 * DuckDB connection lifecycle for server-side parquet queries.
 * Uses @duckdb/node-api for N-API bindings via Deno's Node compat.
 */
import { DuckDBInstance } from "@duckdb/node-api";

// Ensure BigInt values are JSON-serializable (DuckDB returns BigInt for integer/timestamp columns)
// deno-lint-ignore no-explicit-any
(BigInt.prototype as any).toJSON = function () {
  return Number(this);
};

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

  // GCS auth: use HMAC credentials from env (GKE), fall back to credential chain (local dev)
  const hmacKeyId = Deno.env.get("GCS_HMAC_KEY_ID");
  const hmacSecret = Deno.env.get("GCS_HMAC_SECRET");
  if (hmacKeyId && hmacSecret) {
    await connection.run(`SET s3_access_key_id='${hmacKeyId}'`);
    await connection.run(`SET s3_secret_access_key='${hmacSecret}'`);
    console.log("DuckDB initialized with GCS access (HMAC credentials)");
  } else {
    try {
      await connection.run("CREATE SECRET (TYPE GCS, PROVIDER CREDENTIAL_CHAIN)");
      console.log("DuckDB initialized with GCS access (credential chain)");
    } catch (e) {
      console.warn("DuckDB GCS auth failed, queries may not work:", (e as Error).message);
    }
  }
}

/** Convert DuckDB values to JSON-safe types (BigInt → Number, nested objects recursed). */
function toSerializable(val: unknown): unknown {
  if (val === null || val === undefined) return val;
  if (typeof val === "bigint") return Number(val);
  if (Array.isArray(val)) return val.map(toSerializable);
  if (typeof val === "object") {
    const obj = val as Record<string, unknown>;
    const out: Record<string, unknown> = {};
    for (const k of Object.keys(obj)) {
      out[k] = toSerializable(obj[k]);
    }
    return out;
  }
  return val;
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
        row[columns[c]] = toSerializable(chunk.getColumnVector(c).getItem(r));
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

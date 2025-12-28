/**
 * PostgreSQL database client using postgres.js
 */
import postgres from "postgres";

let sql: ReturnType<typeof postgres> | null = null;

/**
 * Get the database connection.
 * Creates a connection pool on first call.
 */
export function getDb(): ReturnType<typeof postgres> {
  if (!sql) {
    const url = Deno.env.get("DATABASE_URL");
    if (!url) {
      throw new Error("DATABASE_URL environment variable not set");
    }
    sql = postgres(url, {
      max: 10,           // Connection pool size
      idle_timeout: 30,  // Close idle connections after 30s
      connect_timeout: 10,
    });
  }
  return sql;
}

/**
 * Close the database connection pool.
 * Call this before shutting down.
 */
export async function closeDb(): Promise<void> {
  if (sql) {
    await sql.end();
    sql = null;
  }
}

/**
 * Execute a query with timing metrics.
 */
export async function withTiming<T>(
  name: string,
  fn: () => Promise<T>
): Promise<{ result: T; durationMs: number }> {
  const start = Date.now();
  const result = await fn();
  const durationMs = Date.now() - start;
  return { result, durationMs };
}

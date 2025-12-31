/**
 * PostgreSQL database client using Drizzle ORM over postgres.js
 */
import { drizzle } from "drizzle-orm/postgres-js";
import postgres from "postgres";
import * as schema from "./schema.ts";

export type Database = ReturnType<typeof drizzle<typeof schema>>;

let db: Database | null = null;
let sql: ReturnType<typeof postgres> | null = null;

/**
 * Get the Drizzle database instance.
 * Creates connection pool on first call.
 */
export function getDb(): Database {
  if (!db) {
    const url = Deno.env.get("DATABASE_URL");
    if (!url) {
      throw new Error("DATABASE_URL environment variable not set");
    }
    sql = postgres(url, {
      max: 10,
      idle_timeout: 30,
      connect_timeout: 10,
    });
    db = drizzle(sql, {
      schema,
      logger: Deno.env.get("DRIZZLE_LOG") === "true"
    });
  }
  return db;
}

/**
 * Get the raw postgres.js client for edge cases.
 * Prefer using getDb() for most queries.
 */
export function getRawSql(): ReturnType<typeof postgres> {
  if (!sql) {
    getDb(); // Initialize if needed
  }
  return sql!;
}

/**
 * Close the database connection pool.
 * Call this before shutting down.
 */
export async function closeDb(): Promise<void> {
  if (sql) {
    await sql.end();
    sql = null;
    db = null;
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

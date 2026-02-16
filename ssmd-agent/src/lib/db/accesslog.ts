/**
 * Data access log operations for audit tracking
 */
import { gte, and, eq, desc } from "drizzle-orm";
import { dataAccessLog, type DataAccessLogEntry, type NewDataAccessLogEntry } from "./schema.ts";
import type { Database } from "./client.ts";

/**
 * Log a data access event (fire-and-forget).
 */
export async function logDataAccess(
  db: Database,
  entry: NewDataAccessLogEntry,
): Promise<void> {
  await db.insert(dataAccessLog).values(entry);
}

/**
 * List recent access log entries since a given date.
 */
export async function listRecentAccess(
  db: Database,
  since: Date,
): Promise<DataAccessLogEntry[]> {
  return db
    .select()
    .from(dataAccessLog)
    .where(gte(dataAccessLog.createdAt, since))
    .orderBy(desc(dataAccessLog.createdAt));
}

/**
 * List access log entries for a specific user since a given date.
 */
export async function listAccessByUser(
  db: Database,
  email: string,
  since: Date,
): Promise<DataAccessLogEntry[]> {
  return db
    .select()
    .from(dataAccessLog)
    .where(
      and(
        eq(dataAccessLog.userEmail, email),
        gte(dataAccessLog.createdAt, since),
      ),
    )
    .orderBy(desc(dataAccessLog.createdAt));
}

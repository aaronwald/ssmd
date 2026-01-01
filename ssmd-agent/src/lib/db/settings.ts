/**
 * Settings database operations for key-value configuration
 */
import { eq } from "drizzle-orm";
import { settings, type Setting } from "./schema.ts";
import type { Database } from "./client.ts";

/**
 * Get a single setting by key.
 */
export async function getSetting(
  db: Database,
  key: string
): Promise<Setting | null> {
  const result = await db
    .select()
    .from(settings)
    .where(eq(settings.key, key))
    .limit(1);

  return result[0] ?? null;
}

/**
 * Get all settings.
 */
export async function getAllSettings(db: Database): Promise<Setting[]> {
  return db.select().from(settings);
}

/**
 * Insert or update a setting (upsert).
 */
export async function upsertSetting(
  db: Database,
  key: string,
  value: unknown
): Promise<Setting> {
  const result = await db
    .insert(settings)
    .values({ key, value, updatedAt: new Date() })
    .onConflictDoUpdate({
      target: settings.key,
      set: { value, updatedAt: new Date() },
    })
    .returning();

  return result[0];
}

/**
 * Get typed setting value with default fallback.
 */
export async function getSettingValue<T>(
  db: Database,
  key: string,
  defaultValue: T
): Promise<T> {
  const setting = await getSetting(db, key);
  return (setting?.value as T) ?? defaultValue;
}

/**
 * Delete a setting by key.
 */
export async function deleteSetting(
  db: Database,
  key: string
): Promise<boolean> {
  const result = await db
    .delete(settings)
    .where(eq(settings.key, key))
    .returning();

  return result.length > 0;
}

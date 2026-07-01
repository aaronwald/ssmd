import { assert, assertEquals } from "jsr:@std/assert";
import { DuckDBInstance } from "@duckdb/node-api";
import { configureDuckDBLimits } from "./hols-duckdb.ts";

/** Read a single scalar setting, failing loudly if DuckDB returns no row. */
async function readSetting(
  conn: Awaited<ReturnType<Awaited<ReturnType<typeof DuckDBInstance.create>>["connect"]>>,
  name: string,
): Promise<string> {
  const reader = await conn.run(`SELECT current_setting('${name}')`);
  const rows = await reader.getRows();
  assert(rows.length > 0 && rows[0].length > 0, `current_setting('${name}') returned no row`);
  const value = rows[0][0];
  assert(value !== null && value !== undefined, `current_setting('${name}') is null`);
  return String(value);
}

Deno.test("configureDuckDBLimits applies memory_limit and temp_directory", async () => {
  const instance = await DuckDBInstance.create();
  const conn = await instance.connect();
  await configureDuckDBLimits(conn, { memoryLimit: "512MB", tempDir: "/tmp/duckdb-spill-test" });

  // DuckDB normalizes "512MB" → "512.0 MiB"
  const mem = await readSetting(conn, "memory_limit");
  assertEquals(mem.replace(/\s+/g, "").toLowerCase().includes("mib"), true);

  const tmp = await readSetting(conn, "temp_directory");
  assertEquals(tmp, "/tmp/duckdb-spill-test");
});

Deno.test("configureDuckDBLimits uses safe defaults below the container limit", async () => {
  const instance = await DuckDBInstance.create();
  const conn = await instance.connect();
  await configureDuckDBLimits(conn);

  const tmp = await readSetting(conn, "temp_directory");
  assertEquals(tmp, "/tmp/duckdb-spill");
});

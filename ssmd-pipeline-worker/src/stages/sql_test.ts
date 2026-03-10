import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { validateSqlQuery, truncateRows } from "./sql.ts";

Deno.test("validateSqlQuery: allows SELECT", () => {
  assertEquals(validateSqlQuery("SELECT * FROM events LIMIT 10"), true);
});

Deno.test("validateSqlQuery: allows WITH (CTE)", () => {
  assertEquals(validateSqlQuery("WITH cte AS (SELECT 1) SELECT * FROM cte"), true);
});

Deno.test("validateSqlQuery: rejects DELETE", () => {
  assertEquals(validateSqlQuery("DELETE FROM events"), false);
});

Deno.test("validateSqlQuery: rejects DROP", () => {
  assertEquals(validateSqlQuery("DROP TABLE events"), false);
});

Deno.test("validateSqlQuery: rejects UPDATE", () => {
  assertEquals(validateSqlQuery("UPDATE events SET title = 'x'"), false);
});

Deno.test("validateSqlQuery: rejects INSERT", () => {
  assertEquals(validateSqlQuery("INSERT INTO events VALUES (1)"), false);
});

Deno.test("validateSqlQuery: rejects TRUNCATE", () => {
  assertEquals(validateSqlQuery("TRUNCATE events"), false);
});

Deno.test("validateSqlQuery: rejects ALTER", () => {
  assertEquals(validateSqlQuery("ALTER TABLE events ADD COLUMN x TEXT"), false);
});

Deno.test("validateSqlQuery: rejects CREATE", () => {
  assertEquals(validateSqlQuery("CREATE TABLE foo (id INT)"), false);
});

Deno.test("validateSqlQuery: rejects semicolon-chained DDL", () => {
  assertEquals(validateSqlQuery("SELECT 1; DROP TABLE events"), false);
});

Deno.test("validateSqlQuery: rejects bare COPY", () => {
  assertEquals(validateSqlQuery("COPY events TO '/tmp/out'"), false);
});

Deno.test("validateSqlQuery: rejects VACUUM", () => {
  assertEquals(validateSqlQuery("VACUUM events"), false);
});

Deno.test("validateSqlQuery: rejects SET", () => {
  assertEquals(validateSqlQuery("SET statement_timeout = 0"), false);
});

Deno.test("validateSqlQuery: rejects multi-statement via semicolon", () => {
  assertEquals(validateSqlQuery("SELECT 1; SELECT 2"), false);
});

Deno.test("truncateRows: respects max_rows", () => {
  const rows = Array.from({ length: 200 }, (_, i) => ({ id: i }));
  const result = truncateRows(rows, 50);
  assertEquals(result.rows.length, 50);
  assertEquals(result.truncated, true);
  assertEquals(result.totalRows, 200);
});

Deno.test("truncateRows: no truncation when under limit", () => {
  const rows = [{ id: 1 }, { id: 2 }];
  const result = truncateRows(rows, 100);
  assertEquals(result.rows.length, 2);
  assertEquals(result.truncated, false);
});

// test/cli/day.test.ts
import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { generateConnectorYaml, generateArchiverYaml } from "../../src/cli/commands/day.ts";

Deno.test("day command - generates correct connector YAML", () => {
  const yaml = generateConnectorYaml("kalshi", "2026-01-04", {
    _: [],
  });

  assertEquals(yaml.includes("name: kalshi-2026-01-04"), true);
  assertEquals(yaml.includes("feed: kalshi"), true);
  assertEquals(yaml.includes('date: "2026-01-04"'), true);
  assertEquals(yaml.includes("kind: Connector"), true);
  assertEquals(yaml.includes("apiVersion: ssmd.ssmd.io/v1alpha1"), true);
  assertEquals(yaml.includes("stream: PROD_KALSHI"), true);
  assertEquals(yaml.includes("subjectPrefix: prod.kalshi"), true);
});

Deno.test("day command - generates correct archiver YAML", () => {
  const yaml = generateArchiverYaml("kalshi", "2026-01-04", {
    _: [],
  });

  assertEquals(yaml.includes("name: kalshi-2026-01-04"), true);
  assertEquals(yaml.includes('date: "2026-01-04"'), true);
  assertEquals(yaml.includes("kind: Archiver"), true);
  assertEquals(yaml.includes("apiVersion: ssmd.ssmd.io/v1alpha1"), true);
  assertEquals(yaml.includes("consumer: archiver-2026-01-04"), true);
  assertEquals(yaml.includes('filter: "prod.kalshi.json.>"'), true);
  // Archiver should NOT have feed in spec (was refactored)
  assertEquals(yaml.includes("spec:\n  feed:"), false);
});

Deno.test("day command - connector uses custom image when provided", () => {
  const yaml = generateConnectorYaml("kalshi", "2026-01-04", {
    _: [],
    "connector-image": "custom/connector:v1.0.0",
  });

  assertEquals(yaml.includes("image: custom/connector:v1.0.0"), true);
});

Deno.test("day command - archiver uses custom image when provided", () => {
  const yaml = generateArchiverYaml("kalshi", "2026-01-04", {
    _: [],
    "archiver-image": "custom/archiver:v2.0.0",
  });

  assertEquals(yaml.includes("image: custom/archiver:v2.0.0"), true);
});

Deno.test("day command - generates correct labels", () => {
  const connectorYaml = generateConnectorYaml("kalshi", "2026-01-05", { _: [] });
  const archiverYaml = generateArchiverYaml("kalshi", "2026-01-05", { _: [] });

  // Both should have feed and date labels
  assertEquals(connectorYaml.includes("ssmd.io/feed: kalshi"), true);
  assertEquals(connectorYaml.includes('ssmd.io/date: "2026-01-05"'), true);
  assertEquals(archiverYaml.includes("ssmd.io/feed: kalshi"), true);
  assertEquals(archiverYaml.includes('ssmd.io/date: "2026-01-05"'), true);
});

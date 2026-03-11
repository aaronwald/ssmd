import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { resolveTemplate } from "./template.ts";
import { computeCronDate } from "./cron.ts";

Deno.test("resolveTemplate: replaces {{input}} with previous output", () => {
  const result = resolveTemplate("Analyze: {{input}}", {
    input: JSON.stringify({ score: 0.8 }),
    stages: {},
    triggerInfo: {},
    date: "2026-03-09",
  });
  assertEquals(result, 'Analyze: {"score":0.8}');
});

Deno.test("resolveTemplate: replaces {{stages.0.output}}", () => {
  const result = resolveTemplate("Results: {{stages.0.output}}", {
    input: "",
    stages: { 0: { output: JSON.stringify({ rows: 5 }) } },
    triggerInfo: {},
    date: "2026-03-09",
  });
  assertEquals(result, 'Results: {"rows":5}');
});

Deno.test("resolveTemplate: replaces {{trigger_info}}", () => {
  const result = resolveTemplate("Alert: {{trigger_info}}", {
    input: "",
    stages: {},
    triggerInfo: { incident: { policy_name: "archiver-idle" } },
    date: "2026-03-09",
  });
  assertEquals(result, 'Alert: {"incident":{"policy_name":"archiver-idle"}}');
});

Deno.test("resolveTemplate: replaces nested {{trigger_info.incident.policy_name}}", () => {
  const result = resolveTemplate("Policy: {{trigger_info.incident.policy_name}}", {
    input: "",
    stages: {},
    triggerInfo: { incident: { policy_name: "archiver-idle" } },
    date: "2026-03-09",
  });
  assertEquals(result, "Policy: archiver-idle");
});

Deno.test("resolveTemplate: replaces {{date}}", () => {
  const result = resolveTemplate("Report for {{date}}", {
    input: "",
    stages: {},
    triggerInfo: {},
    date: "2026-03-09",
  });
  assertEquals(result, "Report for 2026-03-09");
});

Deno.test("resolveTemplate: single-pass — does not re-evaluate substituted values", () => {
  const result = resolveTemplate("Value: {{input}}", {
    input: "{{date}}",
    stages: {},
    triggerInfo: {},
    date: "2026-03-09",
  });
  assertEquals(result, "Value: {{date}}");
});

Deno.test("resolveTemplate: unknown placeholder left as-is", () => {
  const result = resolveTemplate("Value: {{unknown}}", {
    input: "",
    stages: {},
    triggerInfo: {},
    date: "2026-03-09",
  });
  assertEquals(result, "Value: {{unknown}}");
});

Deno.test("resolveTemplate: missing stage returns empty string", () => {
  const result = resolveTemplate("Value: {{stages.5.output}}", {
    input: "",
    stages: {},
    triggerInfo: {},
    date: "2026-03-09",
  });
  assertEquals(result, "Value: ");
});

Deno.test("resolveTemplate: {{stages.N.output.field}} extracts nested string field", () => {
  const result = resolveTemplate("Analysis: {{stages.1.output.content}}", {
    input: "",
    stages: { 1: { output: JSON.stringify({ content: "STATUS: GREEN", model: "claude", usage: {} }) } },
    triggerInfo: {},
    date: "2026-03-09",
  });
  assertEquals(result, "Analysis: STATUS: GREEN");
});

Deno.test("resolveTemplate: {{stages.N.output.field}} extracts nested object as JSON", () => {
  const result = resolveTemplate("Body: {{stages.0.output.body}}", {
    input: "",
    stages: { 0: { output: JSON.stringify({ status: 200, body: { rows: 5, tickers: 45 }, truncated: false }) } },
    triggerInfo: {},
    date: "2026-03-09",
  });
  assertEquals(result, 'Body: {"rows":5,"tickers":45}');
});

Deno.test("resolveTemplate: {{stages.N.output.deep.path}} extracts deeply nested field", () => {
  const result = resolveTemplate("BTC: {{stages.0.output.body.rest.btc_avg_close}}", {
    input: "",
    stages: { 0: { output: JSON.stringify({ body: { rest: { btc_avg_close: 67586.05 } } }) } },
    triggerInfo: {},
    date: "2026-03-09",
  });
  assertEquals(result, "BTC: 67586.05");
});

Deno.test("resolveTemplate: {{stages.N.output.missing}} returns empty for missing field", () => {
  const result = resolveTemplate("Val: {{stages.0.output.nonexistent}}", {
    input: "",
    stages: { 0: { output: JSON.stringify({ status: 200 }) } },
    triggerInfo: {},
    date: "2026-03-09",
  });
  assertEquals(result, "Val: ");
});

Deno.test("resolveTemplate: {{stages.N.output.field}} returns empty for non-JSON output", () => {
  const result = resolveTemplate("Val: {{stages.0.output.field}}", {
    input: "",
    stages: { 0: { output: "plain text" } },
    triggerInfo: {},
    date: "2026-03-09",
  });
  assertEquals(result, "Val: ");
});

Deno.test("resolveTemplate: {{date}} uses trigger_info.date override when present", () => {
  const result = resolveTemplate("Report for {{date}}", {
    input: "",
    stages: {},
    triggerInfo: { trigger: "manual", date: "2026-03-09" },
    date: "2026-03-09",
  });
  assertEquals(result, "Report for 2026-03-09");
});

// ── computeCronDate tests ───────────────────────────────────────

Deno.test("computeCronDate: defaults to T-1 when no date_offset_days", () => {
  const now = new Date("2026-03-11T01:30:00Z");
  assertEquals(computeCronDate({}, now), "2026-03-10");
});

Deno.test("computeCronDate: respects explicit date_offset_days = -1", () => {
  const now = new Date("2026-03-11T01:30:00Z");
  assertEquals(computeCronDate({ date_offset_days: -1 }, now), "2026-03-10");
});

Deno.test("computeCronDate: date_offset_days = 0 gives today", () => {
  const now = new Date("2026-03-11T01:30:00Z");
  assertEquals(computeCronDate({ date_offset_days: 0 }, now), "2026-03-11");
});

Deno.test("computeCronDate: date_offset_days = -2 gives day before yesterday", () => {
  const now = new Date("2026-03-11T01:30:00Z");
  assertEquals(computeCronDate({ date_offset_days: -2 }, now), "2026-03-09");
});

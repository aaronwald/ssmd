import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { resolveTemplate } from "./template.ts";

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

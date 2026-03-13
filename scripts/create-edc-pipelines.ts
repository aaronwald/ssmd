#!/usr/bin/env -S deno run --allow-net
/**
 * Create EDC (Exchange Driven Change) pipeline definitions via data-ts API.
 *
 * Usage:
 *   deno run --allow-net scripts/create-edc-pipelines.ts <base_url> <admin_api_key>
 *
 * Example:
 *   deno run --allow-net scripts/create-edc-pipelines.ts http://localhost:8081 sk-admin-xxx
 */

const EXCHANGES = [
  { name: "kalshi", label: "Kalshi", schedule: "0 10 * * *" },
  { name: "kraken", label: "Kraken", schedule: "15 10 * * *" },
  { name: "binance", label: "Binance", schedule: "30 10 * * *" },
] as const;

const SYSTEM_PROMPT = `You are an EDC (Exchange Driven Change) analyst for the ssmd market data platform. Your job is to analyze exchange API changelog updates and determine their impact on our system.

Our system components:
- connector (Rust): WebSocket client per exchange, publishes raw JSON to NATS
- archiver (Rust): Consumes NATS, writes JSONL.gz archives to GCS
- parquet-gen (Rust): Converts JSONL.gz archives to Parquet files with typed schemas per message type
- snap (Rust): Writes latest ticker/trade to Redis with 5-min TTL, stores raw exchange JSON
- data-ts (Deno): HTTP API server, reads snap data from Redis, enriches monitor endpoints
- harman (Rust): OMS — submits orders, processes fills, tracks positions via exchange REST/WS APIs
- ssmd-cache (Rust): Warms Redis from Postgres secmaster data via CDC

Data flow: Exchange WS -> Connector -> NATS -> Archiver -> GCS -> Parquet-gen
                                    -> Snap -> Redis -> data-ts API -> harman-web UI

When analyzing changes, consider:
1. WebSocket message format changes (field names, types, new/removed fields)
2. REST API endpoint changes (new versions, deprecations, auth changes)
3. Rate limit changes
4. New product types or market structures
5. Fee schedule changes

Output your analysis as:
- CHANGES: List each distinct change detected
- SEVERITY: none | low | medium | high | critical
- AFFECTED_COMPONENTS: Which of our components need updates
- EVIDENCE: Why you believe these components are affected
- RECOMMENDED_ACTIONS: Specific steps to take
- MEMORIES: Reference any relevant past EDC memories that inform your analysis`;

function buildPipeline(exchange: string, label: string, schedule: string) {
  const userPrompt = `Exchange: ${exchange}

== CHANGELOG CHANGE DETECTED ==
Changed: {{stages.0.output.body.changed}}

== NEW CHANGELOG TEXT (latest) ==
{{stages.0.output.body.new_text}}

== PREVIOUS CHANGELOG TEXT ==
{{stages.0.output.body.old_text}}

== PAST EDC MEMORIES (what broke before and how we fixed it) ==
{{stages.1.output.rows}}

== SYSTEM CONTEXT ==
{{stages.2.output.rows}}

== CURRENT SCHEMA VERSIONS ==
{{stages.3.output.body}}

When you detect changes that could alter field semantics (type changes, field renames, unit changes), flag them with:
- SCHEMA_VERSION_IMPACT: Which schemas are affected and why a version bump may be needed

If changed is false, respond with exactly: "NO_CHANGE: Changelog unchanged, no action needed."
Otherwise, analyze the differences between the previous and current changelog. Identify what changed and assess impact on our system.`;

  return {
    name: `edc-${exchange}`,
    description: `EDC: Monitor ${label} changelog for API changes`,
    trigger_type: "cron",
    trigger_config: { schedule },
    stages: [
      {
        name: "Fetch changelog",
        stage_type: "http",
        config: {
          url: `http://ssmd-data-ts-internal:8081/v1/internal/changelog-fetch?exchange=${exchange}`,
          method: "GET",
        },
      },
      {
        name: "Load memories",
        stage_type: "sql",
        config: {
          query: `SELECT id, changelog_summary, impact, affected_components, fix_description, created_at::text FROM edc_memories WHERE exchange = '${exchange}' ORDER BY created_at DESC LIMIT 20`,
          max_rows: 20,
        },
      },
      {
        name: "Load system context",
        stage_type: "sql",
        config: {
          query: "SELECT 'feeds' AS context_type, string_agg(DISTINCT feed, ', ') AS value FROM (SELECT 'kalshi' AS feed UNION SELECT 'kraken-futures' UNION SELECT 'kraken-spot' UNION SELECT 'binance') f UNION ALL SELECT 'active_markets', COUNT(*)::text FROM markets WHERE status = 'active'",
          max_rows: 10,
        },
      },
      {
        name: "Fetch schema versions",
        stage_type: "http",
        config: {
          url: "http://ssmd-data-ts-internal:8081/v1/data/schema-versions",
          method: "GET",
        },
      },
      {
        name: "LLM analysis",
        stage_type: "openrouter",
        config: {
          model: "anthropic/claude-sonnet-4.6",
          system_prompt: SYSTEM_PROMPT,
          user_prompt: userPrompt,
          max_tokens: 3000,
          temperature: 0.3,
        },
      },
      {
        name: "Verification: data freshness",
        stage_type: "http",
        config: {
          url: "http://ssmd-data-ts-internal:8081/v1/data/freshness",
          method: "GET",
        },
      },
      {
        name: "Email report",
        stage_type: "email",
        config: {
          to: "aaronwald@gmail.com",
          subject: `EDC Report: ${exchange} — {{date}}`,
          html: `<h1>EDC Report: ${exchange}</h1><p><strong>Date:</strong> {{date}}</p><p><strong>Changelog Changed:</strong> {{stages.0.output.body.changed}}</p><h2>LLM Analysis</h2><pre style="white-space: pre-wrap;">{{stages.4.output.content}}</pre><h2>Verification: Data Freshness</h2><pre style="white-space: pre-wrap;">{{stages.5.output.body}}</pre>`,
        },
      },
    ],
  };
}

async function main() {
  const [baseUrl, adminApiKey] = Deno.args;

  if (!baseUrl || !adminApiKey) {
    console.error("Usage: deno run --allow-net scripts/create-edc-pipelines.ts <base_url> <admin_api_key>");
    Deno.exit(1);
  }

  const url = `${baseUrl.replace(/\/$/, "")}/v1/pipelines`;
  let failed = false;

  for (const { name, label, schedule } of EXCHANGES) {
    const pipeline = buildPipeline(name, label, schedule);
    console.log(`\nCreating pipeline: ${pipeline.name}`);

    try {
      const resp = await fetch(url, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          "X-API-Key": adminApiKey,
        },
        body: JSON.stringify(pipeline),
      });

      const body = await resp.json();

      if (!resp.ok) {
        console.error(`  FAILED (${resp.status}):`, JSON.stringify(body, null, 2));
        failed = true;
        continue;
      }

      console.log(`  Created: id=${body.id}, name=${body.name}`);
      if (body.webhook_secret) {
        console.log(`  Webhook secret: ${body.webhook_secret}`);
      }
      console.log(`  Trigger: cron ${schedule}`);
      console.log(`  Stages: ${pipeline.stages.length}`);
    } catch (err) {
      console.error(`  ERROR: ${err}`);
      failed = true;
    }
  }

  if (failed) {
    console.error("\nSome pipelines failed to create.");
    Deno.exit(1);
  }

  console.log("\nAll EDC pipelines created successfully.");
}

main();

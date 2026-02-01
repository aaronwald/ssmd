/**
 * K8s Job Runner for parameter sweeps.
 *
 * Creates ConfigMaps, submits Jobs in batches respecting maxParallel,
 * polls for completion, collects results, and cleans up.
 */

import { stringify as stringifyYaml } from "https://deno.land/std@0.224.0/yaml/mod.ts";
import type { GeneratedConfig } from "./spec.ts";
import type { SweepResult } from "./results.ts";
import { parseSummaryJson } from "./results.ts";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface SweepRunOptions {
  sweepRunId: string;
  configs: GeneratedConfig[];
  dateRange: { from: string; to: string };
  image: string;
  maxParallel: number;
}

interface JobStatus {
  configId: string;
  runId: string;
  succeeded: number;
  failed: number;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const decoder = new TextDecoder();

/** Run kubectl and return stdout. Throws on non-zero exit. */
async function kubectl(args: string[]): Promise<string> {
  const cmd = new Deno.Command("kubectl", {
    args,
    stdout: "piped",
    stderr: "piped",
  });
  const output = await cmd.output();
  if (!output.success) {
    const stderr = decoder.decode(output.stderr);
    throw new Error(`kubectl ${args.slice(0, 3).join(" ")} failed: ${stderr}`);
  }
  return decoder.decode(output.stdout);
}

/** Pipe YAML to kubectl via stdin. */
async function kubectlApply(yaml: string): Promise<void> {
  const cmd = new Deno.Command("kubectl", {
    args: ["apply", "-f", "-"],
    stdin: "piped",
    stdout: "piped",
    stderr: "piped",
  });
  const child = cmd.spawn();
  const writer = child.stdin.getWriter();
  await writer.write(new TextEncoder().encode(yaml));
  await writer.close();
  const output = await child.output();
  if (!output.success) {
    const stderr = decoder.decode(output.stderr);
    throw new Error(`kubectl apply failed: ${stderr}`);
  }
}

/** Truncate a K8s resource name to 63 chars (DNS label limit). */
function truncateName(name: string): string {
  if (name.length <= 63) return name;
  return name.slice(0, 63).replace(/-+$/, "");
}

// ---------------------------------------------------------------------------
// ConfigMap creation
// ---------------------------------------------------------------------------

function buildConfigMapYaml(
  sweepRunId: string,
  configId: string,
  config: Record<string, unknown>,
): string {
  const yamlContent = stringifyYaml(config);
  const name = truncateName(`sweep-${sweepRunId}-${configId}`);
  return `apiVersion: v1
kind: ConfigMap
metadata:
  name: ${name}
  namespace: ssmd
  labels:
    app: ssmd-backtest
    sweep-run: "${sweepRunId}"
    config-id: "${configId}"
data:
  momentum.yaml: |
${yamlContent.split("\n").map((l) => `    ${l}`).join("\n")}
`;
}

async function createConfigMaps(
  sweepRunId: string,
  configs: GeneratedConfig[],
): Promise<void> {
  for (const cfg of configs) {
    const yaml = buildConfigMapYaml(
      sweepRunId,
      cfg.configId,
      cfg.config as unknown as Record<string, unknown>,
    );
    await kubectlApply(yaml);
  }
}

// ---------------------------------------------------------------------------
// Job creation
// ---------------------------------------------------------------------------

function buildJobYaml(
  sweepRunId: string,
  configId: string,
  runId: string,
  dateRange: { from: string; to: string },
  image: string,
): string {
  const jobName = truncateName(`sweep-${sweepRunId}-${configId}`);
  const cmName = truncateName(`sweep-${sweepRunId}-${configId}`);
  const resultsDir = `/results/sweeps/${sweepRunId}/${configId}`;

  return `apiVersion: batch/v1
kind: Job
metadata:
  name: ${jobName}
  namespace: ssmd
  labels:
    app: ssmd-backtest
    sweep-run: "${sweepRunId}"
    config-id: "${configId}"
    run-id: "${runId}"
spec:
  backoffLimit: 0
  ttlSecondsAfterFinished: 86400
  template:
    metadata:
      labels:
        app: ssmd-backtest
        sweep-run: "${sweepRunId}"
        config-id: "${configId}"
        run-id: "${runId}"
    spec:
      restartPolicy: Never
      securityContext:
        runAsUser: 1000
        runAsGroup: 1000
        fsGroup: 1000
      imagePullSecrets:
        - name: ghcr-secret
      containers:
        - name: backtest
          image: ghcr.io/aaronwald/ssmd-backtest:${image}
          args:
            - "--config"
            - "/config/momentum.yaml"
            - "--from"
            - "${dateRange.from}"
            - "--to"
            - "${dateRange.to}"
            - "--cache-dir"
            - "/cache"
            - "--results-dir"
            - "${resultsDir}"
            - "--run-id"
            - "${runId}"
          env:
            - name: GOOGLE_APPLICATION_CREDENTIALS
              value: /secrets/gcs/key.json
          resources:
            requests:
              cpu: 100m
              memory: 512Mi
            limits:
              cpu: "1"
              memory: 1Gi
          volumeMounts:
            - name: config
              mountPath: /config
              readOnly: true
            - name: cache
              mountPath: /cache
            - name: results
              mountPath: /results
            - name: gcs-credentials
              mountPath: /secrets/gcs
              readOnly: true
      volumes:
        - name: config
          configMap:
            name: ${cmName}
        - name: cache
          persistentVolumeClaim:
            claimName: ssmd-backtest-cache
        - name: results
          persistentVolumeClaim:
            claimName: ssmd-backtest-results
        - name: gcs-credentials
          secret:
            secretName: gcs-credentials
`;
}

async function submitJob(
  sweepRunId: string,
  configId: string,
  dateRange: { from: string; to: string },
  image: string,
): Promise<string> {
  const runId = crypto.randomUUID();
  const yaml = buildJobYaml(sweepRunId, configId, runId, dateRange, image);
  await kubectlApply(yaml);
  return runId;
}

// ---------------------------------------------------------------------------
// Polling
// ---------------------------------------------------------------------------

async function pollJobs(sweepRunId: string): Promise<JobStatus[]> {
  const jsonPath =
    '{range .items[*]}{.metadata.labels.config-id}{"\\t"}{.metadata.labels.run-id}{"\\t"}{.status.succeeded}{"\\t"}{.status.failed}{"\\n"}{end}';
  const raw = await kubectl([
    "get", "jobs",
    "-n", "ssmd",
    "-l", `sweep-run=${sweepRunId}`,
    "-o", `jsonpath=${jsonPath}`,
  ]);

  const statuses: JobStatus[] = [];
  for (const line of raw.trim().split("\n")) {
    if (!line.trim()) continue;
    const [configId, runId, succeededStr, failedStr] = line.split("\t");
    statuses.push({
      configId: configId ?? "",
      runId: runId ?? "",
      succeeded: parseInt(succeededStr ?? "0", 10) || 0,
      failed: parseInt(failedStr ?? "0", 10) || 0,
    });
  }
  return statuses;
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

// ---------------------------------------------------------------------------
// runSweep — main orchestration
// ---------------------------------------------------------------------------

export async function runSweep(opts: SweepRunOptions): Promise<void> {
  const { sweepRunId, configs, dateRange, image, maxParallel } = opts;
  const total = configs.length;

  console.log(`[sweep] Starting sweep ${sweepRunId} — ${total} configs, maxParallel=${maxParallel}`);

  // 1. Create all ConfigMaps up-front
  console.log(`[sweep] Creating ${total} ConfigMaps...`);
  await createConfigMaps(sweepRunId, configs);

  // 2. Track submission state
  const queue = [...configs]; // configs waiting to be submitted
  const active = new Map<string, string>(); // configId -> runId
  let completed = 0;
  let failed = 0;

  // Submit initial batch
  while (queue.length > 0 && active.size < maxParallel) {
    const cfg = queue.shift()!;
    const runId = await submitJob(sweepRunId, cfg.configId, dateRange, image);
    active.set(cfg.configId, runId);
  }

  console.log(`[sweep] Submitted initial batch of ${active.size} jobs`);

  // 3. Poll loop
  while (active.size > 0) {
    await sleep(10_000);
    const statuses = await pollJobs(sweepRunId);

    for (const st of statuses) {
      if (!active.has(st.configId)) continue;

      const done = st.succeeded > 0 || st.failed > 0;
      if (!done) continue;

      // Job finished
      active.delete(st.configId);
      if (st.succeeded > 0) {
        completed++;
        console.log(`[sweep] [${completed + failed}/${total}] ${st.configId} → OK`);
      } else {
        failed++;
        console.log(`[sweep] [${completed + failed}/${total}] ${st.configId} → FAILED`);
      }

      // Submit next from queue
      if (queue.length > 0) {
        const next = queue.shift()!;
        const runId = await submitJob(sweepRunId, next.configId, dateRange, image);
        active.set(next.configId, runId);
      }
    }
  }

  // 4. Summary
  console.log(`[sweep] Sweep ${sweepRunId} complete: ${completed} succeeded, ${failed} failed out of ${total}`);
}

// ---------------------------------------------------------------------------
// collectResults
// ---------------------------------------------------------------------------

export async function collectResults(
  sweepRunId: string,
  configs: GeneratedConfig[],
): Promise<SweepResult[]> {
  const results: SweepResult[] = [];

  for (const cfg of configs) {
    try {
      const json = await kubectl([
        "exec", "-n", "ssmd",
        "deploy/ssmd-debug", "--",
        "cat", `/results/sweeps/${sweepRunId}/${cfg.configId}/summary.json`,
      ]);
      results.push(parseSummaryJson(json, cfg.configId, cfg.params));
    } catch {
      results.push({
        configId: cfg.configId,
        params: cfg.params,
        trades: 0,
        wins: 0,
        losses: 0,
        winRate: 0,
        netPnl: 0,
        maxDrawdown: 0,
        halted: false,
        status: "failed",
        error: "no results found",
      });
    }
  }

  return results;
}

// ---------------------------------------------------------------------------
// cleanupSweep
// ---------------------------------------------------------------------------

export async function cleanupSweep(sweepRunId: string): Promise<void> {
  console.log(`[sweep] Cleaning up sweep ${sweepRunId}...`);

  // Delete Jobs
  await kubectl([
    "delete", "jobs",
    "-n", "ssmd",
    "-l", `sweep-run=${sweepRunId}`,
    "--ignore-not-found",
  ]);

  // Delete ConfigMaps
  await kubectl([
    "delete", "configmaps",
    "-n", "ssmd",
    "-l", `sweep-run=${sweepRunId}`,
    "--ignore-not-found",
  ]);

  console.log(`[sweep] Cleanup complete for ${sweepRunId}`);
}

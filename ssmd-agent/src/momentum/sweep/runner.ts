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
  timeoutMinutes?: number;
}

interface JobStatus {
  configIndex: string;
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

/** Build a K8s-safe resource name using a zero-padded index. */
function resourceName(sweepRunId: string, index: number): string {
  return `sweep-${sweepRunId}-${String(index).padStart(4, "0")}`;
}

// ---------------------------------------------------------------------------
// ConfigMap creation
// ---------------------------------------------------------------------------

function buildConfigMapYaml(
  sweepRunId: string,
  index: number,
  configId: string,
  config: Record<string, unknown>,
): string {
  const yamlContent = stringifyYaml(config);
  const name = resourceName(sweepRunId, index);
  return `apiVersion: v1
kind: ConfigMap
metadata:
  name: ${name}
  namespace: ssmd
  labels:
    app: ssmd-backtest
    sweep-run: "${sweepRunId}"
    config-index: "${index}"
data:
  momentum.yaml: |
${yamlContent.split("\n").map((l) => `    ${l}`).join("\n")}
`;
}

async function createConfigMaps(
  sweepRunId: string,
  configs: GeneratedConfig[],
): Promise<void> {
  for (let i = 0; i < configs.length; i++) {
    const yaml = buildConfigMapYaml(
      sweepRunId,
      i,
      configs[i].configId,
      configs[i].config as unknown as Record<string, unknown>,
    );
    await kubectlApply(yaml);
  }
}

// ---------------------------------------------------------------------------
// Job creation
// ---------------------------------------------------------------------------

function buildJobYaml(
  sweepRunId: string,
  index: number,
  configId: string,
  runId: string,
  dateRange: { from: string; to: string },
  image: string,
): string {
  const jobName = resourceName(sweepRunId, index);
  const cmName = resourceName(sweepRunId, index);
  const resultsDir = `/results/sweeps/${sweepRunId}/${configId}`;

  return `apiVersion: batch/v1
kind: Job
metadata:
  name: ${jobName}
  namespace: ssmd
  labels:
    app: ssmd-backtest
    sweep-run: "${sweepRunId}"
    config-index: "${index}"
    run-id: "${runId}"
spec:
  backoffLimit: 0
  ttlSecondsAfterFinished: 120
  template:
    metadata:
      labels:
        app: ssmd-backtest
        sweep-run: "${sweepRunId}"
        config-index: "${index}"
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
  index: number,
  configId: string,
  dateRange: { from: string; to: string },
  image: string,
): Promise<string> {
  const runId = crypto.randomUUID();
  const yaml = buildJobYaml(sweepRunId, index, configId, runId, dateRange, image);
  await kubectlApply(yaml);
  return runId;
}

// ---------------------------------------------------------------------------
// Polling
// ---------------------------------------------------------------------------

async function pollJobs(sweepRunId: string): Promise<JobStatus[]> {
  const jsonPath =
    '{range .items[*]}{.metadata.labels.config-index}{"\\t"}{.metadata.labels.run-id}{"\\t"}{.status.succeeded}{"\\t"}{.status.failed}{"\\n"}{end}';
  const raw = await kubectl([
    "get", "jobs",
    "-n", "ssmd",
    "-l", `sweep-run=${sweepRunId}`,
    "-o", `jsonpath=${jsonPath}`,
  ]);

  const statuses: JobStatus[] = [];
  for (const line of raw.trim().split("\n")) {
    if (!line.trim()) continue;
    const [configIndex, runId, succeededStr, failedStr] = line.split("\t");
    statuses.push({
      configIndex: configIndex ?? "",
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

const DEFAULT_TIMEOUT_MINUTES = 360; // 6 hours

export async function runSweep(opts: SweepRunOptions): Promise<void> {
  const { sweepRunId, configs, dateRange, image, maxParallel } = opts;
  const timeoutMs = (opts.timeoutMinutes ?? DEFAULT_TIMEOUT_MINUTES) * 60 * 1000;
  const total = configs.length;

  console.log(`[sweep] Starting sweep ${sweepRunId} — ${total} configs, maxParallel=${maxParallel}`);

  // 1. Create all ConfigMaps up-front
  console.log(`[sweep] Creating ${total} ConfigMaps...`);
  await createConfigMaps(sweepRunId, configs);

  // 2. Track submission state — keyed by string index
  interface QueueEntry { index: number; configId: string }
  const queue: QueueEntry[] = configs.map((c, i) => ({ index: i, configId: c.configId }));
  const active = new Map<string, string>(); // index (as string) -> runId
  let completed = 0;
  let failed = 0;

  // Submit initial batch
  while (queue.length > 0 && active.size < maxParallel) {
    const entry = queue.shift()!;
    const runId = await submitJob(sweepRunId, entry.index, entry.configId, dateRange, image);
    active.set(String(entry.index), runId);
  }

  console.log(`[sweep] Submitted initial batch of ${active.size} jobs`);

  // 3. Poll loop with timeout
  const startTime = Date.now();
  while (active.size > 0) {
    if (Date.now() - startTime > timeoutMs) {
      console.error(`[sweep] Timeout after ${opts.timeoutMinutes ?? DEFAULT_TIMEOUT_MINUTES} minutes — ${active.size} jobs still running`);
      break;
    }

    await sleep(10_000);
    const statuses = await pollJobs(sweepRunId);

    for (const st of statuses) {
      if (!active.has(st.configIndex)) continue;

      const done = st.succeeded > 0 || st.failed > 0;
      if (!done) continue;

      // Job finished
      const idx = parseInt(st.configIndex, 10);
      const cfgId = configs[idx]?.configId ?? st.configIndex;
      active.delete(st.configIndex);
      if (st.succeeded > 0) {
        completed++;
        console.log(`[sweep] [${completed + failed}/${total}] ${cfgId} → OK`);
      } else {
        failed++;
        console.log(`[sweep] [${completed + failed}/${total}] ${cfgId} → FAILED`);
      }

      // Submit next from queue
      if (queue.length > 0) {
        const next = queue.shift()!;
        const runId = await submitJob(sweepRunId, next.index, next.configId, dateRange, image);
        active.set(String(next.index), runId);
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
      // Backtest writes to {resultsDir}/{runId}/summary.json — find the latest run-id subdir
      const configDir = `/results/sweeps/${sweepRunId}/${cfg.configId}`;
      const json = await kubectl([
        "exec", "-n", "ssmd",
        "deploy/ssmd-debug", "--",
        "sh", "-c", `latest=$(ls -t ${configDir} | head -1) && cat ${configDir}/$latest/summary.json`,
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

"use client";

import { useState, useMemo } from "react";
import { useParams, useRouter } from "next/navigation";
import Link from "next/link";
import {
  usePipeline,
  usePipelineRuns,
  usePipelineRunDetail,
  useMe,
} from "@/lib/hooks";
import { triggerPipeline, updatePipeline, deletePipeline } from "@/lib/api";
import type {
  Pipeline,
  PipelineStage,
  PipelineRun,
  PipelineRunStatus,
  PipelineStageResult,
} from "@/lib/types";
import { mutate } from "swr";

function RunStatusBadge({ status }: { status: PipelineRunStatus | null | undefined }) {
  if (!status) return <span className="text-xs text-fg-subtle">-</span>;
  let cls = "bg-fg-subtle/15 text-fg-subtle";
  if (status === "completed") cls = "bg-green/15 text-green";
  else if (status === "running" || status === "pending") cls = "bg-yellow/15 text-yellow";
  else if (status === "failed") cls = "bg-red/15 text-red";
  return (
    <span className={`text-xs px-1.5 py-0.5 rounded font-medium ${cls}`}>
      {status}
    </span>
  );
}

function fmtDate(d: string | null) {
  if (!d) return "-";
  return new Date(d).toLocaleString([], {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function fmtDuration(start: string | null, end: string | null) {
  if (!start || !end) return "-";
  const ms = new Date(end).getTime() - new Date(start).getTime();
  if (ms < 1000) return `${ms}ms`;
  return `${(ms / 1000).toFixed(1)}s`;
}

export default function PipelineDetailPage() {
  const { data: me } = useMe();
  const hasAdmin = me?.scopes.includes("harman:admin") || me?.scopes.includes("*");

  if (!me) return <div className="py-10 text-center text-fg-muted">Loading...</div>;
  if (!hasAdmin) return <div className="py-10 text-center text-fg-muted">Requires <code className="font-mono text-accent">harman:admin</code> scope.</div>;

  return <PipelineDetail />;
}

function PipelineDetail() {
  const params = useParams();
  const router = useRouter();
  const id = params.id ? Number(params.id) : null;
  const { data: pipeline, error } = usePipeline(id);
  const { data: runs } = usePipelineRuns(id);
  const [running, setRunning] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [dateOverride, setDateOverride] = useState("");

  if (error) return <p className="text-sm text-red">Error: {error.message}</p>;
  if (!pipeline) return <div className="py-10 text-center text-fg-muted">Loading...</div>;

  async function handleRun() {
    if (!id) return;
    setRunning(true);
    try {
      const payload = dateOverride ? { date: dateOverride } : undefined;
      await triggerPipeline(id, payload);
      await mutate(`data-pipeline-runs-${id}`);
      await mutate(`data-pipeline-${id}`);
    } catch {
      // shown on next refresh
    } finally {
      setRunning(false);
    }
  }

  async function handleDelete() {
    if (!id || !confirm("Delete this pipeline?")) return;
    setDeleting(true);
    try {
      await deletePipeline(id);
      await mutate("data-pipelines");
      router.push("/pipelines");
    } catch {
      setDeleting(false);
    }
  }

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-start justify-between">
        <div>
          <div className="flex items-center gap-2 mb-1">
            <Link href="/pipelines" className="text-xs text-fg-subtle hover:text-accent transition-colors">&larr; Pipelines</Link>
          </div>
          <h1 className="text-xl font-bold">{pipeline.name}</h1>
          {pipeline.description && <p className="text-sm text-fg-muted mt-1">{pipeline.description}</p>}
        </div>
        <div className="flex items-center gap-2">
          <input
            type="date"
            value={dateOverride}
            onChange={(e) => setDateOverride(e.target.value)}
            className="px-2 py-1 text-xs rounded border border-border bg-bg text-fg font-mono w-[130px]"
            title="Date override (leave empty for today)"
          />
          <button
            onClick={handleRun}
            disabled={running}
            className={`px-3 py-1.5 text-xs rounded border transition-colors ${
              running ? "border-border text-fg-subtle cursor-wait" : "border-accent/50 text-accent hover:bg-accent/10"
            }`}
          >
            {running ? "Running..." : "Run Now"}
          </button>
          <button
            onClick={handleDelete}
            disabled={deleting}
            className="px-3 py-1.5 text-xs rounded border border-red/30 text-red hover:bg-red/10 transition-colors"
          >
            Delete
          </button>
        </div>
      </div>

      {/* Config cards */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        <TriggerCard pipeline={pipeline} />
        <InfoCard pipeline={pipeline} />
      </div>

      {/* Stages */}
      {pipeline.stages && pipeline.stages.length > 0 && (
        <StagesSection stages={pipeline.stages} />
      )}

      {/* Run history */}
      <RunHistory runs={runs} pipelineId={id!} />
    </div>
  );
}

function TriggerCard({ pipeline: p }: { pipeline: Pipeline }) {
  return (
    <div className="bg-bg-raised border border-border rounded-lg p-4 space-y-3">
      <h2 className="text-sm font-semibold text-fg">Trigger</h2>
      <div className="space-y-2 text-xs">
        <div className="flex justify-between">
          <span className="text-fg-muted">Type</span>
          <span className="font-mono text-fg">{p.trigger_type}</span>
        </div>
        {p.trigger_type === "cron" && p.trigger_config?.schedule != null && (
          <div className="flex justify-between">
            <span className="text-fg-muted">Schedule</span>
            <span className="font-mono text-fg">{String(p.trigger_config.schedule)}</span>
          </div>
        )}
        {p.trigger_type === "webhook" && p.webhook_secret && (
          <div className="space-y-1">
            <span className="text-fg-muted">Webhook URL</span>
            <WebhookUrl pipelineId={p.id} secret={p.webhook_secret} />
          </div>
        )}
        {p.trigger_config && Object.keys(p.trigger_config).length > 0 && (
          <details className="text-xs">
            <summary className="text-fg-subtle cursor-pointer hover:text-fg-muted">Raw config</summary>
            <pre className="mt-1 p-2 bg-bg rounded text-fg-subtle font-mono text-[11px] overflow-x-auto">
              {JSON.stringify(p.trigger_config, null, 2)}
            </pre>
          </details>
        )}
      </div>
    </div>
  );
}

function WebhookUrl({ pipelineId, secret }: { pipelineId: number; secret: string }) {
  const [copied, setCopied] = useState(false);
  const url = `https://api.varshtat.com/v1/pipelines/${pipelineId}/trigger?secret=${secret}`;

  function copy() {
    navigator.clipboard.writeText(url);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  }

  return (
    <div className="flex items-center gap-1">
      <code className="block flex-1 p-1.5 bg-bg rounded text-[11px] font-mono text-fg-muted break-all select-all">
        {url}
      </code>
      <button
        onClick={copy}
        className="shrink-0 px-2 py-1 text-[10px] rounded border border-border text-fg-subtle hover:text-fg transition-colors"
      >
        {copied ? "Copied" : "Copy"}
      </button>
    </div>
  );
}

function InfoCard({ pipeline: p }: { pipeline: Pipeline }) {
  return (
    <div className="bg-bg-raised border border-border rounded-lg p-4 space-y-3">
      <h2 className="text-sm font-semibold text-fg">Info</h2>
      <div className="space-y-2 text-xs">
        <div className="flex justify-between">
          <span className="text-fg-muted">ID</span>
          <span className="font-mono text-fg">{p.id}</span>
        </div>
        <div className="flex justify-between">
          <span className="text-fg-muted">Enabled</span>
          <span className={p.enabled ? "text-green" : "text-fg-subtle"}>{p.enabled ? "Yes" : "No"}</span>
        </div>
        <div className="flex justify-between">
          <span className="text-fg-muted">Last Run</span>
          <RunStatusBadge status={p.last_run_status} />
        </div>
        <div className="flex justify-between">
          <span className="text-fg-muted">Created</span>
          <span className="font-mono text-fg-muted">{fmtDate(p.created_at)}</span>
        </div>
        <div className="flex justify-between">
          <span className="text-fg-muted">Updated</span>
          <span className="font-mono text-fg-muted">{fmtDate(p.updated_at)}</span>
        </div>
      </div>
    </div>
  );
}

function StagesSection({ stages }: { stages: PipelineStage[] }) {
  const sorted = useMemo(() => [...stages].sort((a, b) => a.position - b.position), [stages]);

  return (
    <div className="space-y-3">
      <h2 className="text-sm font-semibold text-fg">Stages ({stages.length})</h2>
      <div className="space-y-2">
        {sorted.map((s, i) => (
          <div key={s.id} className="bg-bg-raised border border-border rounded-lg p-3 flex items-start gap-3">
            <span className="shrink-0 w-6 h-6 rounded-full bg-accent/15 text-accent text-xs font-bold flex items-center justify-center mt-0.5">
              {i + 1}
            </span>
            <div className="flex-1 min-w-0">
              <div className="flex items-center gap-2">
                <span className="text-sm font-medium text-fg">{s.name}</span>
                <span className="text-xs font-mono px-1.5 py-0.5 rounded bg-fg-subtle/15 text-fg-subtle">{s.stage_type}</span>
              </div>
              <details className="mt-1 text-xs">
                <summary className="text-fg-subtle cursor-pointer hover:text-fg-muted">Config</summary>
                <pre className="mt-1 p-2 bg-bg rounded text-fg-subtle font-mono text-[11px] overflow-x-auto">
                  {JSON.stringify(s.config, null, 2)}
                </pre>
              </details>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

function RunHistory({ runs, pipelineId }: { runs: PipelineRun[] | undefined; pipelineId: number }) {
  const [expandedRun, setExpandedRun] = useState<number | null>(null);

  return (
    <div className="space-y-3">
      <h2 className="text-sm font-semibold text-fg">Run History</h2>
      {!runs ? (
        <p className="text-xs text-fg-muted">Loading...</p>
      ) : runs.length === 0 ? (
        <p className="text-xs text-fg-subtle">No runs yet</p>
      ) : (
        <div className="bg-bg-raised border border-border rounded-lg overflow-hidden">
          <table className="w-full text-sm">
            <thead>
              <tr className="text-left text-xs text-fg-muted border-b border-border">
                <th className="px-4 py-2">Run</th>
                <th className="px-4 py-2">Status</th>
                <th className="px-4 py-2">Started</th>
                <th className="px-4 py-2">Duration</th>
                <th className="px-4 py-2">Trigger</th>
              </tr>
            </thead>
            <tbody>
              {runs.map((r) => (
                <RunRow
                  key={r.id}
                  run={r}
                  isExpanded={expandedRun === r.id}
                  onToggle={() => setExpandedRun(expandedRun === r.id ? null : r.id)}
                />
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

function RunRow({ run, isExpanded, onToggle }: { run: PipelineRun; isExpanded: boolean; onToggle: () => void }) {
  return (
    <>
      <tr
        className={`border-b border-border-subtle cursor-pointer transition-colors ${
          isExpanded ? "bg-bg-surface" : "hover:bg-bg-surface-hover"
        }`}
        onClick={onToggle}
      >
        <td className="px-4 py-2 font-mono text-fg-muted">#{run.id}</td>
        <td className="px-4 py-2"><RunStatusBadge status={run.status} /></td>
        <td className="px-4 py-2 text-xs font-mono text-fg-muted">{fmtDate(run.started_at || run.created_at)}</td>
        <td className="px-4 py-2 text-xs font-mono text-fg-muted">{fmtDuration(run.started_at, run.finished_at)}</td>
        <td className="px-4 py-2 text-xs text-fg-subtle">
          {run.trigger_info?.type ? String(run.trigger_info.type) : "-"}
        </td>
      </tr>
      {isExpanded && (
        <tr>
          <td colSpan={5} className="px-4 py-3 bg-bg">
            <RunDetail runId={run.id} />
          </td>
        </tr>
      )}
    </>
  );
}

function RunDetail({ runId }: { runId: number }) {
  const { data: run, error } = usePipelineRunDetail(runId);

  if (error) return <p className="text-xs text-red">Error: {error.message}</p>;
  if (!run) return <p className="text-xs text-fg-muted">Loading stage results...</p>;

  const results = run.stage_results || [];

  if (results.length === 0) {
    return <p className="text-xs text-fg-subtle">No stage results</p>;
  }

  return (
    <div className="space-y-2">
      {results.map((sr, i) => (
        <StageResultCard key={sr.id} result={sr} index={i} />
      ))}
    </div>
  );
}

function StageResultCard({ result: sr, index }: { result: PipelineStageResult; index: number }) {
  const [showOutput, setShowOutput] = useState(false);

  return (
    <div className="border border-border rounded p-3 space-y-2">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className="text-xs font-mono text-fg-muted">Stage {index + 1}</span>
          <RunStatusBadge status={sr.status} />
        </div>
        <span className="text-xs font-mono text-fg-subtle">
          {fmtDuration(sr.started_at, sr.finished_at)}
        </span>
      </div>

      {sr.error && (
        <pre className="text-xs text-red bg-red/5 p-2 rounded font-mono overflow-x-auto">
          {sr.error}
        </pre>
      )}

      {sr.output != null && (
        <div>
          <button
            onClick={() => setShowOutput(!showOutput)}
            className="text-xs text-fg-subtle hover:text-fg-muted transition-colors"
          >
            {showOutput ? "Hide output" : "Show output"}
          </button>
          {showOutput && (
            <OutputDisplay output={sr.output} />
          )}
        </div>
      )}
    </div>
  );
}

function OutputDisplay({ output }: { output: unknown }) {
  if (typeof output === "string") {
    // Check if it looks like LLM/markdown content
    if (output.length > 200 || output.includes("\n")) {
      return (
        <pre className="mt-1 p-2 bg-bg rounded text-xs text-fg-muted font-mono overflow-x-auto whitespace-pre-wrap max-h-96 overflow-y-auto">
          {output}
        </pre>
      );
    }
    return <span className="text-xs text-fg-muted font-mono">{output}</span>;
  }

  return (
    <pre className="mt-1 p-2 bg-bg rounded text-[11px] text-fg-subtle font-mono overflow-x-auto max-h-96 overflow-y-auto">
      {JSON.stringify(output, null, 2)}
    </pre>
  );
}

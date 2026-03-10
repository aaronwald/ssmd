"use client";

import { useState } from "react";
import Link from "next/link";
import { usePipelines, useMe } from "@/lib/hooks";
import { triggerPipeline, updatePipeline } from "@/lib/api";
import type { Pipeline, PipelineRunStatus } from "@/lib/types";
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

function TriggerBadge({ type }: { type: string }) {
  const cls = type === "cron"
    ? "bg-accent/15 text-accent"
    : "bg-fg-subtle/15 text-fg-muted";
  return (
    <span className={`text-xs px-1.5 py-0.5 rounded font-medium font-mono ${cls}`}>
      {type}
    </span>
  );
}

export default function PipelinesPage() {
  const { data: me } = useMe();
  const hasAdmin = me?.scopes.includes("harman:admin") || me?.scopes.includes("*");

  if (!me) return <div className="py-10 text-center text-fg-muted">Loading...</div>;
  if (!hasAdmin) return <div className="py-10 text-center text-fg-muted">Requires <code className="font-mono text-accent">harman:admin</code> scope.</div>;

  return <PipelinesContent />;
}

function PipelinesContent() {
  const { data: pipelines, error } = usePipelines();

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-bold">Pipelines</h1>
        {pipelines && <span className="text-xs text-fg-muted">{pipelines.length} pipeline{pipelines.length !== 1 ? "s" : ""}</span>}
      </div>

      {error && <p className="text-sm text-red">Error: {error.message}</p>}
      {!pipelines && !error && <p className="text-sm text-fg-muted">Loading...</p>}

      {pipelines && (
        <div className="bg-bg-raised border border-border rounded-lg overflow-hidden">
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="text-left text-xs text-fg-muted border-b border-border">
                  <th className="px-4 py-2">Name</th>
                  <th className="px-4 py-2">Trigger</th>
                  <th className="px-4 py-2">Last Run</th>
                  <th className="px-4 py-2">Enabled</th>
                  <th className="px-4 py-2">Last Triggered</th>
                  <th className="px-4 py-2"></th>
                </tr>
              </thead>
              <tbody>
                {pipelines.length > 0 ? pipelines.map((p) => (
                  <PipelineRow key={p.id} pipeline={p} />
                )) : (
                  <tr><td colSpan={6} className="px-4 py-8 text-center text-fg-subtle text-sm">No pipelines configured</td></tr>
                )}
              </tbody>
            </table>
          </div>
        </div>
      )}
    </div>
  );
}

function PipelineRow({ pipeline: p }: { pipeline: Pipeline }) {
  const [running, setRunning] = useState(false);
  const [toggling, setToggling] = useState(false);

  async function handleRun() {
    setRunning(true);
    try {
      await triggerPipeline(p.id);
      await mutate("data-pipelines");
    } catch {
      // error will show on next refresh
    } finally {
      setRunning(false);
    }
  }

  async function handleToggle() {
    setToggling(true);
    try {
      await updatePipeline(p.id, { enabled: !p.enabled });
      await mutate("data-pipelines");
    } catch {
      // error will show on next refresh
    } finally {
      setToggling(false);
    }
  }

  return (
    <tr className="border-b border-border-subtle hover:bg-bg-surface-hover">
      <td className="px-4 py-2">
        <Link href={`/pipelines/${p.id}`} className="text-fg hover:text-accent transition-colors font-medium">
          {p.name}
        </Link>
        {p.description && <p className="text-xs text-fg-subtle mt-0.5 max-w-[300px] truncate">{p.description}</p>}
      </td>
      <td className="px-4 py-2">
        <TriggerBadge type={p.trigger_type} />
        {p.trigger_type === "cron" && p.trigger_config?.schedule != null && (
          <span className="ml-1.5 text-xs text-fg-subtle font-mono">{String(p.trigger_config.schedule)}</span>
        )}
      </td>
      <td className="px-4 py-2">
        <RunStatusBadge status={p.last_run_status} />
      </td>
      <td className="px-4 py-2">
        <button
          onClick={handleToggle}
          disabled={toggling}
          className={`relative inline-flex h-5 w-9 items-center rounded-full transition-colors ${
            p.enabled ? "bg-green" : "bg-fg-subtle/30"
          } ${toggling ? "opacity-50" : ""}`}
          title={p.enabled ? "Disable" : "Enable"}
        >
          <span className={`inline-block h-3.5 w-3.5 rounded-full bg-white transition-transform ${
            p.enabled ? "translate-x-4.5" : "translate-x-0.5"
          }`} />
        </button>
      </td>
      <td className="px-4 py-2 text-xs text-fg-muted font-mono whitespace-nowrap">
        {p.last_triggered_at
          ? new Date(p.last_triggered_at).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" })
          : "-"}
      </td>
      <td className="px-4 py-2">
        <button
          onClick={handleRun}
          disabled={running}
          className={`px-2.5 py-1 text-xs rounded border transition-colors ${
            running
              ? "border-border text-fg-subtle cursor-wait"
              : "border-accent/50 text-accent hover:bg-accent/10"
          }`}
        >
          {running ? "Running..." : "Run Now"}
        </button>
      </td>
    </tr>
  );
}

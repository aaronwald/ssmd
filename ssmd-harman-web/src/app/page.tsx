"use client";

import { useState } from "react";
import { StatusDot } from "@/components/status-dot";
import { RiskGauge } from "@/components/risk-gauge";
import { useHealth, usePositions, useRisk } from "@/lib/hooks";
import { reconcile, resume, massCancel } from "@/lib/api";

export default function Dashboard() {
  const { data: health } = useHealth();
  const { data: positions } = usePositions();
  const { data: risk } = useRisk();
  const [actionMsg, setActionMsg] = useState("");

  async function runAction(label: string, fn: () => Promise<void>) {
    setActionMsg("");
    try {
      await fn();
      setActionMsg(`${label} completed`);
    } catch (err) {
      setActionMsg(`${label} failed: ${err instanceof Error ? err.message : "unknown error"}`);
    }
  }

  const healthStatus = health
    ? health.status === "ok"
      ? "green"
      : "red"
    : "yellow";

  return (
    <div className="space-y-6">
      <h1 className="text-xl font-bold">Dashboard</h1>

      {/* Health + Risk row */}
      <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
        <div className="bg-bg-raised border border-border rounded-lg p-4 space-y-3">
          <h2 className="text-sm font-medium text-fg-muted">Health</h2>
          <StatusDot status={healthStatus} />
          {health && (
            <div className="text-xs text-fg-muted space-y-1">
              <div>Session: <span className="font-mono text-fg">{health.session_state}</span></div>
              <div>Uptime: <span className="font-mono text-fg">{Math.floor(health.uptime_seconds)}s</span></div>
            </div>
          )}
        </div>

        <div className="bg-bg-raised border border-border rounded-lg p-4 space-y-3">
          <h2 className="text-sm font-medium text-fg-muted">Risk</h2>
          {risk ? <RiskGauge risk={risk} /> : <span className="text-xs text-fg-subtle">Loading...</span>}
        </div>
      </div>

      {/* Quick actions */}
      <div className="bg-bg-raised border border-border rounded-lg p-4 space-y-3">
        <h2 className="text-sm font-medium text-fg-muted">Quick Actions</h2>
        <div className="flex gap-3">
          <button onClick={() => runAction("Reconcile", reconcile)} className="rounded-md bg-accent px-4 py-1.5 text-sm font-medium text-fg hover:bg-accent-hover transition-colors">
            Reconcile
          </button>
          <button onClick={() => runAction("Resume", resume)} className="rounded-md bg-green/20 text-green px-4 py-1.5 text-sm font-medium hover:bg-green/30 transition-colors">
            Resume
          </button>
          <button onClick={() => runAction("Mass Cancel", massCancel)} className="rounded-md bg-red/20 text-red px-4 py-1.5 text-sm font-medium hover:bg-red/30 transition-colors">
            Mass Cancel
          </button>
        </div>
        {actionMsg && <p className="text-xs text-fg-muted">{actionMsg}</p>}
      </div>

      {/* Positions */}
      <div className="bg-bg-raised border border-border rounded-lg p-4 space-y-3">
        <h2 className="text-sm font-medium text-fg-muted">Positions</h2>
        {positions && positions.length > 0 ? (
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="text-left text-xs text-fg-muted border-b border-border">
                  <th className="pb-2 pr-4">Ticker</th>
                  <th className="pb-2 pr-4 text-right">Exchange</th>
                  <th className="pb-2 pr-4 text-right">Local</th>
                  <th className="pb-2">Match</th>
                </tr>
              </thead>
              <tbody>
                {positions.map((p) => (
                  <tr key={p.ticker} className="border-b border-border-subtle">
                    <td className="py-2 pr-4 font-mono">{p.ticker}</td>
                    <td className="py-2 pr-4 font-mono text-right">{p.exchange_position}</td>
                    <td className="py-2 pr-4 font-mono text-right">{p.local_position}</td>
                    <td className="py-2">
                      {p.mismatch ? (
                        <span className="text-red text-xs font-medium">MISMATCH</span>
                      ) : (
                        <span className="text-green text-xs font-medium">OK</span>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <p className="text-xs text-fg-subtle">No positions</p>
        )}
      </div>
    </div>
  );
}

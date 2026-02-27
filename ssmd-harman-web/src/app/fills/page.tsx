"use client";

import { useState } from "react";
import { useFills, useAudit } from "@/lib/hooks";
import { StateBadge } from "@/components/state-badge";
import type { OrderState } from "@/lib/types";

type Tab = "fills" | "audit";

export default function FillsPage() {
  const [tab, setTab] = useState<Tab>("fills");
  const { data: fills, error: fillsErr } = useFills();
  const { data: audit, error: auditErr } = useAudit();

  return (
    <div className="space-y-6">
      <div className="flex items-center gap-4">
        <h1 className="text-xl font-bold">Fills & Audit</h1>
        <div className="flex gap-1">
          <button
            onClick={() => setTab("fills")}
            className={`rounded-md px-3 py-1 text-sm transition-colors ${tab === "fills" ? "bg-accent text-fg" : "text-fg-muted hover:text-fg"}`}
          >
            Fills
          </button>
          <button
            onClick={() => setTab("audit")}
            className={`rounded-md px-3 py-1 text-sm transition-colors ${tab === "audit" ? "bg-accent text-fg" : "text-fg-muted hover:text-fg"}`}
          >
            Audit Log
          </button>
        </div>
      </div>

      {tab === "fills" && (
        <>
          {fillsErr && <p className="text-sm text-red">Error: {fillsErr.message}</p>}
          <div className="bg-bg-raised border border-border rounded-lg overflow-hidden">
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className="text-left text-xs text-fg-muted border-b border-border">
                    <th className="px-4 py-2">Order ID</th>
                    <th className="px-4 py-2">Ticker</th>
                    <th className="px-4 py-2">Side</th>
                    <th className="px-4 py-2">Action</th>
                    <th className="px-4 py-2 text-right">Price</th>
                    <th className="px-4 py-2 text-right">Qty</th>
                    <th className="px-4 py-2">Taker</th>
                    <th className="px-4 py-2">Filled At</th>
                  </tr>
                </thead>
                <tbody>
                  {fills && fills.length > 0 ? (
                    fills.map((f) => (
                      <tr key={f.id} className="border-b border-border-subtle hover:bg-bg-surface-hover">
                        <td className="px-4 py-2 font-mono text-fg-muted">{f.order_id}</td>
                        <td className="px-4 py-2 font-mono">{f.ticker}</td>
                        <td className="px-4 py-2 uppercase">{f.side}</td>
                        <td className="px-4 py-2 uppercase">{f.action}</td>
                        <td className="px-4 py-2 font-mono text-right">${f.price}</td>
                        <td className="px-4 py-2 font-mono text-right">{f.quantity}</td>
                        <td className="px-4 py-2">{f.is_taker ? <span className="text-yellow text-xs">Yes</span> : <span className="text-fg-subtle text-xs">No</span>}</td>
                        <td className="px-4 py-2 text-xs text-fg-muted">{new Date(f.filled_at).toLocaleString()}</td>
                      </tr>
                    ))
                  ) : (
                    <tr>
                      <td colSpan={8} className="px-4 py-8 text-center text-fg-subtle text-sm">
                        {fills ? "No fills" : "Loading..."}
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
          </div>
        </>
      )}

      {tab === "audit" && (
        <>
          {auditErr && <p className="text-sm text-red">Error: {auditErr.message}</p>}
          <div className="bg-bg-raised border border-border rounded-lg overflow-hidden">
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className="text-left text-xs text-fg-muted border-b border-border">
                    <th className="px-4 py-2">Order</th>
                    <th className="px-4 py-2">Group</th>
                    <th className="px-4 py-2">Event</th>
                    <th className="px-4 py-2">Detail</th>
                    <th className="px-4 py-2">Time</th>
                  </tr>
                </thead>
                <tbody>
                  {audit && audit.length > 0 ? (
                    audit.map((a) => {
                      const stateMatch = a.detail.match(/(\w+)\s*->\s*(\w+)/);
                      return (
                        <tr key={a.id} className="border-b border-border-subtle hover:bg-bg-surface-hover">
                          <td className="px-4 py-2 font-mono text-fg-muted">{a.order_id ?? "-"}</td>
                          <td className="px-4 py-2 font-mono text-fg-muted">{a.group_id ?? "-"}</td>
                          <td className="px-4 py-2 font-mono text-xs">{a.event_type}</td>
                          <td className="px-4 py-2 text-xs">
                            {stateMatch ? (
                              <span className="inline-flex items-center gap-1">
                                <StateBadge state={stateMatch[1] as OrderState} />
                                <span className="text-fg-subtle">-&gt;</span>
                                <StateBadge state={stateMatch[2] as OrderState} />
                              </span>
                            ) : (
                              <span className="text-fg-muted">{a.detail}</span>
                            )}
                          </td>
                          <td className="px-4 py-2 text-xs text-fg-muted">{new Date(a.created_at).toLocaleString()}</td>
                        </tr>
                      );
                    })
                  ) : (
                    <tr>
                      <td colSpan={5} className="px-4 py-8 text-center text-fg-subtle text-sm">
                        {audit ? "No audit entries" : "Loading..."}
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
          </div>
        </>
      )}
    </div>
  );
}
